//! Review › Compare: the differences between the open presentation and a revised copy of it, slide
//! by slide, and accepting a revised slide into the open one.
//!
//! Slides match by id (a revised copy of the same deck keeps them); when the two decks share no
//! slide id at all, by position. Shapes match by id, then by name.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_model::{Fill, MediaId, Presentation, Shape, ShapeId, ShapeKind, Slide, SlideId};
use serde::Serialize;

/// The revised presentation a document is being compared with.
#[derive(Clone, Debug)]
pub struct Compare {
    pub name: String,
    pub revised: Arc<Presentation>,
    /// Slides match by position (the decks shared no slide id when the compare started); fixed
    /// for the session so accepting slides doesn't switch the matching half way.
    pub by_position: bool,
}

/// Whether `cur` and `rev` share no slide id (then slides match by position).
pub fn unrelated(cur: &Presentation, rev: &Presentation) -> bool {
    let ids: HashSet<SlideId> = rev.slides.iter().map(|s| s.id).collect();
    !cur.slides.iter().any(|s| ids.contains(&s.id))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    /// Only in the revised deck.
    Inserted,
    /// Only in the open deck.
    Deleted,
    /// In both, different.
    Changed,
}

/// One slide-level revision.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub kind: ChangeKind,
    /// Index in the open deck (for inserted: where it would go).
    pub slide: usize,
    /// Index in the revised deck (none for deleted).
    pub revised: Option<usize>,
    /// What differs, readable: `Title 1: text`, `Picture 3 inserted`, `notes`…
    pub details: Vec<String>,
}

const MAX_DETAILS: usize = 50;

/// Slide pairs (open index, revised index).
fn pairs(cur: &Presentation, rev: &Presentation, by_position: bool) -> Vec<(usize, usize)> {
    if by_position {
        return (0..cur.slides.len().min(rev.slides.len())).map(|i| (i, i)).collect();
    }
    let rev_ids: HashMap<SlideId, usize> = rev.slides.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    cur.slides.iter().enumerate().filter_map(|(i, s)| rev_ids.get(&s.id).map(|j| (i, *j))).collect()
}

pub fn diff(cur: &Presentation, rev: &Presentation, by_position: bool) -> Vec<Change> {
    let pairs = pairs(cur, rev, by_position);
    let cur_matched: HashSet<usize> = pairs.iter().map(|p| p.0).collect();
    let rev_matched: HashSet<usize> = pairs.iter().map(|p| p.1).collect();
    let mut out = vec![];
    for (i, j) in &pairs {
        let (Some(a), Some(b)) = (cur.slides.get(*i), rev.slides.get(*j)) else { continue };
        let details = slide_details(cur, a, rev, b);
        if !details.is_empty() {
            out.push(Change { kind: ChangeKind::Changed, slide: *i, revised: Some(*j), details });
        }
    }
    for (i, s) in cur.slides.iter().enumerate() {
        if !cur_matched.contains(&i) {
            out.push(Change { kind: ChangeKind::Deleted, slide: i, revised: None, details: vec![slide_title(s)] });
        }
    }
    for (j, s) in rev.slides.iter().enumerate() {
        if !rev_matched.contains(&j) {
            // After the open slide that matches the revised slide before it.
            let at = (0..j).rev().find_map(|k| pairs.iter().find(|p| p.1 == k).map(|p| p.0 + 1)).unwrap_or(0);
            out.push(Change { kind: ChangeKind::Inserted, slide: at, revised: Some(j), details: vec![slide_title(s)] });
        }
    }
    out.sort_by_key(|c| (c.slide, c.revised.unwrap_or(0)));
    out
}

fn slide_title(s: &Slide) -> String {
    let t = s
        .shapes
        .iter()
        .find(|x| x.ph.as_ref().is_some_and(|p| matches!(p.kind, deckcraft_model::PhType::Title | deckcraft_model::PhType::CtrTitle)))
        .and_then(|x| x.text.as_ref())
        .map(|b| b.text())
        .unwrap_or_default();
    let t = t.trim();
    if t.is_empty() { "Untitled slide".into() } else { t.chars().take(60).collect() }
}

fn shape_label(s: &Shape) -> String {
    if s.name.trim().is_empty() { format!("Shape {}", s.id.0) } else { s.name.clone() }
}

fn media_bytes(p: &Presentation, id: MediaId) -> Option<&[u8]> {
    p.media(id).map(|m| m.data.as_slice())
}

/// A shape without its ids (shape and media), for comparing across decks.
fn plain_shape(s: &Shape) -> Shape {
    let mut c = s.clone();
    fn rec(shapes: &mut [Shape]) {
        for s in shapes {
            s.id = ShapeId(0);
            if let Some(ch) = s.children_mut() {
                rec(ch);
            }
        }
    }
    rec(std::slice::from_mut(&mut c));
    c
}

fn shape_details(cp: &Presentation, a: &Shape, rp: &Presentation, b: &Shape, out: &mut Vec<String>) {
    let label = shape_label(b);
    let text = |s: &Shape| s.text.as_ref().map(|t| t.text()).unwrap_or_default();
    let mut what = vec![];
    if text(a) != text(b) {
        what.push("text");
    } else if a.text != b.text {
        what.push("text formatting");
    }
    match (&a.xfrm, &b.xfrm) {
        (Some(x), Some(y)) if (x.x, x.y) != (y.x, y.y) && (x.w, x.h) == (y.w, y.h) => what.push("moved"),
        (Some(x), Some(y)) if (x.w, x.h) != (y.w, y.h) => what.push("resized"),
        (x, y) if x.as_ref().map(|x| x.rot) != y.as_ref().map(|y| y.rot) => what.push("rotated"),
        _ => {}
    }
    if a.fill != b.fill || a.line != b.line || a.style != b.style || a.effects != b.effects {
        what.push("formatting");
    }
    let pic = |p: &Presentation, s: &Shape| match &s.kind {
        ShapeKind::Picture { fill } => media_bytes(p, fill.media).map(|b| b.to_vec()),
        _ => None,
    };
    if pic(cp, a) != pic(rp, b) {
        what.push("picture");
    }
    if what.is_empty() {
        let (mut x, mut y) = (plain_shape(a), plain_shape(b));
        x.text = None;
        y.text = None;
        if let (ShapeKind::Picture { .. }, ShapeKind::Picture { .. }) = (&x.kind, &y.kind) {
            x.kind = ShapeKind::Shape;
            y.kind = ShapeKind::Shape;
        }
        if x != y {
            what.push("properties");
        }
    }
    if !what.is_empty() {
        out.push(format!("{label}: {}", what.join(", ")));
    }
}

fn slide_details(cp: &Presentation, a: &Slide, rp: &Presentation, b: &Slide) -> Vec<String> {
    let mut out = vec![];
    let mut used: HashSet<usize> = HashSet::new();
    for sa in &a.shapes {
        let k = b
            .shapes
            .iter()
            .position(|x| x.id == sa.id)
            .or_else(|| b.shapes.iter().enumerate().position(|(k, x)| !used.contains(&k) && x.name == sa.name && !x.name.is_empty()));
        match k {
            Some(k) if !used.contains(&k) => {
                used.insert(k);
                if let Some(sb) = b.shapes.get(k) {
                    shape_details(cp, sa, rp, sb, &mut out);
                }
            }
            _ => out.push(format!("{} deleted", shape_label(sa))),
        }
    }
    for (k, sb) in b.shapes.iter().enumerate() {
        if !used.contains(&k) {
            out.push(format!("{} inserted", shape_label(sb)));
        }
    }
    let order = |s: &Slide| s.shapes.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    if out.is_empty() && order(a) != order(b) {
        out.push("order of objects".into());
    }
    let layout_name = |p: &Presentation, s: &Slide| p.layout(s.layout).map(|(_, l)| l.name.clone());
    if layout_name(cp, a) != layout_name(rp, b) {
        out.push("layout".into());
    }
    if a.notes.text() != b.notes.text() {
        out.push("notes".into());
    }
    if a.transition != b.transition {
        out.push("transition".into());
    }
    if a.animations.len() != b.animations.len()
        || a.animations
            .iter()
            .zip(&b.animations)
            .any(|(x, y)| (&x.effect, x.class, x.start, x.duration_ms, x.delay_ms) != (&y.effect, y.class, y.start, y.duration_ms, y.delay_ms))
    {
        out.push("animations".into());
    }
    if a.background != b.background {
        out.push("background".into());
    }
    if a.hidden != b.hidden {
        out.push(if b.hidden { "hidden" } else { "shown" }.into());
    }
    out.truncate(MAX_DETAILS);
    out
}

fn max_shape_id(shapes: &[Shape], m: &mut u32) {
    deckcraft_model::walk(shapes, &mut |s, _| *m = (*m).max(s.id.0));
}

/// The revised slide made to live in `doc`: its media copied over (remapped on an id clash), shape
/// ids kept unless another slide of `doc` uses them (then fresh, animations following), and its
/// layout matched by id, then by name, then the first layout. An error when `doc` has used every
/// object id and cannot take the slide without duplicating ids (#407).
pub fn adopt(doc: &mut Presentation, rev: &Presentation, slide: &Slide, replacing: Option<SlideId>) -> crate::Result<Slide> {
    let mut sl = slide.clone();
    // Media.
    let mut mids = vec![];
    crate::cmd::edit::media_of_shapes(&sl.shapes, &mut mids);
    let mut media_map: HashMap<MediaId, MediaId> = HashMap::new();
    for id in mids {
        let Some(m) = rev.media(id) else { continue };
        match doc.media(id) {
            Some(x) if x.data == m.data => {}
            None => doc.media.push(m.clone()),
            Some(_) => {
                let new = doc
                    .add_media(&m.name, &m.content_type, m.data.as_ref().clone())
                    .ok_or_else(|| crate::cmd::bad("review.accept", crate::cmd::NO_ID_SPACE))?;
                media_map.insert(id, new);
            }
        }
    }
    // Shape ids used elsewhere in the open deck.
    let mut taken: HashSet<u32> = HashSet::new();
    for s in doc.slides.iter().filter(|s| Some(s.id) != replacing) {
        deckcraft_model::walk(&s.shapes, &mut |x, _| {
            taken.insert(x.id.0);
        });
    }
    let mut id_map: HashMap<ShapeId, ShapeId> = HashMap::new();
    fn rec(
        doc: &mut Presentation,
        shapes: &mut [Shape],
        taken: &HashSet<u32>,
        ids: &mut HashMap<ShapeId, ShapeId>,
        media: &HashMap<MediaId, MediaId>,
        depth: usize,
    ) -> crate::Result<()> {
        if depth > 64 {
            return Ok(());
        }
        for s in shapes {
            if taken.contains(&s.id.0) {
                let new = ShapeId(doc.alloc_id().ok_or_else(|| crate::cmd::bad("review.accept", crate::cmd::NO_ID_SPACE))?);
                ids.insert(s.id, new);
                s.id = new;
            }
            let remap = |m: &mut MediaId| {
                if let Some(n) = media.get(m) {
                    *m = *n;
                }
            };
            match &mut s.kind {
                ShapeKind::Picture { fill } => remap(&mut fill.media),
                ShapeKind::Media(m) => {
                    remap(&mut m.media);
                    if let Some(p) = m.poster.as_mut() {
                        remap(p);
                    }
                }
                _ => {}
            }
            if let Some(Fill::Picture(pf)) = &mut s.fill {
                remap(&mut pf.media);
            }
            if let Some(ch) = s.children_mut() {
                rec(doc, ch, taken, ids, media, depth + 1)?;
            }
        }
        Ok(())
    }
    rec(doc, &mut sl.shapes, &taken, &mut id_map, &media_map, 0)?;
    for a in &mut sl.animations {
        if let Some(n) = id_map.get(&a.shape) {
            a.shape = *n;
        }
        if let Some(t) = a.trigger.as_mut()
            && let Some(n) = id_map.get(t)
        {
            *t = *n;
        }
    }
    let mut max = 0;
    max_shape_id(&sl.shapes, &mut max);
    doc.next_id = doc.next_id.max(max);
    // Layout.
    if doc.layout(sl.layout).is_none() {
        let name = rev.layout(slide.layout).map(|(_, l)| l.name.clone());
        let by_name = name.and_then(|n| doc.masters.iter().flat_map(|m| &m.layouts).find(|l| l.name == n).map(|l| l.id));
        sl.layout = by_name.or_else(|| doc.masters.first().and_then(|m| m.layouts.first()).map(|l| l.id)).unwrap_or_default();
    }
    match replacing {
        Some(id) => sl.id = id,
        None => {
            if doc.slides.iter().any(|s| s.id == sl.id) || sl.id.0 == 0 {
                sl.id = SlideId(doc.alloc_id().ok_or_else(|| crate::cmd::bad("review.accept", crate::cmd::NO_ID_SPACE))?);
            }
            doc.next_id = doc.next_id.max(sl.id.0);
        }
    }
    Ok(sl)
}

/// Apply `change` (from [`diff`] of `doc` against `rev`) to `doc`. Returns the slide index to show
/// (`None` when the change no longer applies), or an error when `doc` has used every object id.
pub fn accept(doc: &mut Presentation, rev: &Presentation, change: &Change) -> crate::Result<Option<usize>> {
    match change.kind {
        ChangeKind::Changed => {
            let Some(b) = change.revised.and_then(|r| rev.slides.get(r)).cloned() else { return Ok(None) };
            let Some(id) = doc.slides.get(change.slide).map(|s| s.id) else { return Ok(None) };
            let sl = adopt(doc, rev, &b, Some(id))?;
            let Some(slot) = doc.slides.get_mut(change.slide) else { return Ok(None) };
            *slot = Arc::new(sl);
            Ok(Some(change.slide))
        }
        ChangeKind::Inserted => {
            let Some(b) = change.revised.and_then(|r| rev.slides.get(r)).cloned() else { return Ok(None) };
            let sl = adopt(doc, rev, &b, None)?;
            let at = change.slide.min(doc.slides.len());
            doc.slides.insert(at, Arc::new(sl));
            Ok(Some(at))
        }
        ChangeKind::Deleted => {
            if change.slide >= doc.slides.len() || doc.slides.len() <= 1 {
                return Ok(None);
            }
            doc.slides.remove(change.slide);
            Ok(Some(change.slide.min(doc.slides.len().saturating_sub(1))))
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    /// The sample deck, and a revised copy of it as base64 `.deckcraft` bytes: slide 1's title
    /// retyped, slide 2 deleted, a new slide at the end, a shape moved on slide 3.
    fn decks() -> (Session, String, usize) {
        let mut s = Session::new();
        crate::sample::open_sample(&mut s).unwrap();
        let n = s.doc().unwrap().doc.slides.len();
        let orig = s.execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
        // Make the revision in a second document.
        s.execute("file.openBytes", &json!({"name": "rev.deckcraft", "data": orig})).unwrap();
        let first = s.doc().unwrap().doc.slides[0].shapes.iter().find(|x| x.text.is_some()).unwrap().id.0;
        s.execute("edit.select", &json!({"ids": [first]})).unwrap();
        s.execute("text.edit", &json!({"id": first})).unwrap();
        s.execute("edit.selectAll", &json!({})).unwrap();
        s.execute("text.insert", &json!({"text": "Revised title"})).unwrap();
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("slide.delete", &json!({"index": 1})).unwrap();
        s.execute("slide.go", &json!({"index": 1})).ok();
        let moved = s.doc().unwrap().doc.slides[1].shapes[0].id.0;
        s.execute("shape.move", &json!({"ids": [moved], "dx": 30, "dy": 0})).ok();
        s.execute("slide.go", &json!({"index": n - 2})).ok();
        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Brand new", "body": "x"})).unwrap();
        let rev = s.execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
        s.close_document(1);
        s.set_active(0);
        (s, rev, n)
    }

    fn kinds(v: &Value) -> Vec<String> {
        v["changes"].as_array().unwrap().iter().map(|c| c["kind"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn lists_inserted_deleted_and_changed_slides() {
        let (mut s, rev, n) = decks();
        let v = s.execute("review.compare", &json!({"name": "rev.deckcraft", "data": rev})).unwrap();
        let k = kinds(&v);
        assert!(k.contains(&"deleted".into()) && k.contains(&"inserted".into()) && k.contains(&"changed".into()), "{v}");
        let ch = &v["changes"].as_array().unwrap()[0];
        assert_eq!(ch["slide"], 0);
        assert!(ch["details"].to_string().contains("text"), "{ch}");
        let ins = v["changes"].as_array().unwrap().iter().find(|c| c["kind"] == "inserted").unwrap();
        assert_eq!(ins["details"][0], "Brand new");
        let at = ins["slide"].as_u64().unwrap() as usize;
        assert_eq!(at, n, "after the open slide matching the revised one before it");
        assert_eq!(s.doc().unwrap().doc.slides.len(), n, "comparing alone changes nothing");
    }

    #[test]
    fn accept_one_then_all_makes_the_decks_equal_and_undoes() {
        let (mut s, rev, n) = decks();
        s.execute("review.compare", &json!({"name": "rev.deckcraft", "data": rev})).unwrap();
        let before = s.execute("review.revisions", &json!({})).unwrap()["changes"].as_array().unwrap().len();
        s.execute("review.accept", &json!({"index": 0})).unwrap();
        let after = s.execute("review.revisions", &json!({})).unwrap()["changes"].as_array().unwrap().len();
        assert_eq!(after, before - 1);
        assert!(s.doc().unwrap().doc.slides[0].shapes.iter().any(|x| x.text.as_ref().is_some_and(|t| t.text().contains("Revised title"))));
        s.execute("review.accept", &json!({"all": true})).unwrap();
        let left = s.execute("review.revisions", &json!({})).unwrap();
        assert!(left["changes"].as_array().unwrap().is_empty(), "{left}");
        assert!(s.doc().unwrap().doc.validate().is_empty(), "{:?}", s.doc().unwrap().doc.validate());
        assert_eq!(s.doc().unwrap().doc.slides.len(), n);
        s.execute("edit.undo", &json!({})).unwrap();
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.execute("review.revisions", &json!({})).unwrap()["changes"].as_array().unwrap().len(), before);
        s.execute("review.endCompare", &json!({})).unwrap();
        assert!(s.execute("review.revisions", &json!({})).is_err());
        assert!(s.execute("review.accept", &json!({"all": true})).is_err());
    }

    #[test]
    fn unrelated_decks_match_by_position_and_bad_input_fails() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "title", "title": "A"})).unwrap();
        let mut other = Session::with_new();
        other.execute("slide.new", &json!({"layout": "title", "title": "B"})).unwrap();
        other.execute("slide.new", &json!({"layout": "title", "title": "C"})).unwrap();
        let data = other.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].as_str().unwrap().to_string();
        let v = s.execute("review.compare", &json!({"name": "o.pptx", "data": data})).unwrap();
        assert!(kinds(&v).contains(&"inserted".into()), "{v}");
        s.execute("review.accept", &json!({"all": true})).unwrap();
        assert!(s.execute("review.revisions", &json!({})).unwrap()["changes"].as_array().unwrap().is_empty());
        assert!(s.doc().unwrap().doc.validate().is_empty());
        assert!(s.execute("review.compare", &json!({"data": "!!"})).is_err());
        assert!(s.execute("review.compare", &json!({"name": "x.bin", "data": "AAAA"})).is_err());
        assert!(s.execute("review.compare", &json!({})).is_err());
        assert!(s.execute("review.accept", &json!({"index": 999})).is_err());
    }
}
