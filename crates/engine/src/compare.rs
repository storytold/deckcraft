//! Review › Compare: the differences between the open presentation and a revised copy of it, slide
//! by slide, and accepting a revised slide into the open one.
//!
//! Slides match by id (a revised copy of the same deck keeps them); when the two decks share no
//! slide id at all, by position. Shapes match by id, then by name.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use deckcraft_model::{
    MediaId, Presentation, PresentationTemplate, PresentationTemplateId, Shape, ShapeId, ShapeKind, Slide, SlideId, presentation_template,
};
use serde::Serialize;

use crate::{
    EngineError, Result,
    template_transfer::{self, TransferMap},
};

/// The revised presentation a document is being compared with.
#[derive(Clone, Debug)]
pub struct Compare {
    pub name: String,
    pub revised: Arc<Presentation>,
    /// Slides match by position (the decks shared no slide id when the compare started); fixed
    /// for the session so accepting slides doesn't switch the matching half way.
    pub by_position: bool,
    /// Proven accepted owners. Snapshots cease to match after edits/undo remove
    /// their destination Arc; equal unrelated payloads never establish a transfer.
    pub(crate) transfers: ReviewTransfers,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ReviewTransfers {
    receipt: TransferMap,
    // Previous accepted destinations may be restored by undo/redo. Keep exact
    // pairs under a fixed logical budget even when currently absent or edited.
    accepted: Vec<(Arc<PresentationTemplate>, Arc<PresentationTemplate>)>,
    // Exact old selected owners intentionally left in the destination pool by
    // accepted page changes. This is not permission to ignore edited unused data.
    retired: Vec<Arc<PresentationTemplate>>,
}

impl ReviewTransfers {
    fn validate_history(&self) -> Result<()> {
        let count = self.accepted.len().checked_add(self.retired.len()).ok_or_else(|| invalid("ownership history count overflow"))?;
        if count > presentation_template::MAX_TEMPLATES {
            return Err(invalid("ownership history count limit exceeded; start a new comparison"));
        }
        let mut bytes = count.saturating_mul(std::mem::size_of::<(Arc<PresentationTemplate>, Arc<PresentationTemplate>)>());
        // Conservative logical accounting counts shared payloads per proof;
        // it is not a claim about exact allocator/RSS usage.
        for owner in self.accepted.iter().flat_map(|(source, destination)| [source, destination]).chain(self.retired.iter()) {
            bytes = bytes
                .saturating_add(std::mem::size_of::<PresentationTemplate>())
                .saturating_add(owner.name.len())
                .saturating_add(owner.display_name.as_ref().map_or(0, String::len))
                .saturating_add(
                    owner.slots.len().saturating_mul(std::mem::size_of::<deckcraft_model::presentation_template::PresentationTemplateSlot>()),
                );
            if bytes > presentation_template::MAX_TEMPLATE_BYTES {
                return Err(invalid("ownership history byte limit exceeded; start a new comparison"));
            }
        }
        Ok(())
    }
}

fn invalid(reason: &str) -> EngineError {
    EngineError::Other(format!("presentation template review: {reason}"))
}

struct TemplateState {
    native_equal: HashSet<PresentationTemplateId>,
    accepted: HashSet<(PresentationTemplateId, PresentationTemplateId)>,
}

impl TemplateState {
    fn new(cur: &Presentation, rev: &Presentation, transfers: &ReviewTransfers) -> Result<Self> {
        transfers.validate_history()?;
        presentation_template::validate(cur)?;
        presentation_template::validate(rev)?;
        // Both pools are bounded by model validation. Build one index, rather
        // than rescanning full resources for every page sharing an owner.
        let current: HashMap<_, _> = cur.presentation_templates.iter().map(|owner| (owner.id, owner)).collect();
        let revised: HashMap<_, _> = rev.presentation_templates.iter().map(|owner| (owner.id, owner)).collect();
        let mut selected = HashSet::new();
        let mut current_selected = HashSet::new();
        selected.try_reserve(revised.len()).map_err(|_| invalid("cannot allocate selected-owner index"))?;
        for slide in &rev.slides {
            if let Some(id) = slide.presentation_template {
                selected.insert(id);
            }
        }
        for slide in &cur.slides {
            if let Some(id) = slide.presentation_template {
                current_selected.insert(id);
            }
        }
        let native_equal: HashSet<_> = current
            .iter()
            .filter_map(|(id, owner)| revised.get(id).is_some_and(|other| Arc::as_ref(owner) == Arc::as_ref(other)).then_some(*id))
            .collect();
        let mut accepted = HashSet::new();
        let mut accepted_destinations = HashSet::new();
        for (source, destination) in transfers.accepted.iter().map(|pair| (&pair.0, &pair.1)).chain(transfers.receipt.pairs()) {
            if revised.get(&source.id).is_some_and(|live| Arc::ptr_eq(live, source))
                && current.get(&destination.id).is_some_and(|live| Arc::ptr_eq(live, destination))
            {
                accepted.insert((source.id, destination.id));
                if selected.contains(&source.id) {
                    accepted_destinations.insert(destination.id);
                }
            }
        }
        let retired: HashSet<_> =
            transfers.retired.iter().filter_map(|owner| current.get(&owner.id).filter(|live| Arc::ptr_eq(live, owner)).map(|_| owner.id)).collect();
        // Review changes are page-owned today. Do not silently ignore added,
        // deleted or edited unused definitions while accepting selected owners.
        if revised.keys().any(|id| !selected.contains(id) && !native_equal.contains(id))
            || current.keys().any(|id| {
                !current_selected.contains(id) && !accepted_destinations.contains(id) && !retired.contains(id) && !native_equal.contains(id)
            })
        {
            return Err(invalid("unused template revisions need resource-level review mapping; review those resources separately"));
        }
        Ok(Self { native_equal, accepted })
    }

    fn same_selection(&self, current: Option<PresentationTemplateId>, revised: Option<PresentationTemplateId>) -> bool {
        match (current, revised) {
            (None, None) => true,
            (Some(current), Some(revised)) => {
                self.accepted.contains(&(revised, current)) || (current == revised && self.native_equal.contains(&current))
            }
            _ => false,
        }
    }
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
    match diff_with(cur, rev, by_position, &ReviewTransfers::default()) {
        Ok(changes) => changes,
        // Preserve the public Vec API while making unsupported comparison
        // visible. Fallible command callers use diff_with and refuse precisely.
        Err(error) => vec![Change { kind: ChangeKind::Changed, slide: 0, revised: None, details: vec![error.to_string()] }],
    }
}

fn notes_review_boundary(cur: &Presentation, rev: &Presentation) -> Result<()> {
    if cur.slides.iter().chain(&rev.slides).any(|slide| slide.notes_page.is_some()) {
        return Err(invalid(
            "structured notes comparison requires owner-aware notes resource review; clipboard and reuse support do not authorize review acceptance",
        ));
    }
    Ok(())
}

pub(crate) fn diff_with(cur: &Presentation, rev: &Presentation, by_position: bool, transfers: &ReviewTransfers) -> Result<Vec<Change>> {
    notes_review_boundary(cur, rev)?;
    deckcraft_model::comments::validate(cur, true).map_err(|e| invalid(&e.to_string()))?;
    deckcraft_model::comments::validate(rev, true).map_err(|e| invalid(&e.to_string()))?;
    let templates = TemplateState::new(cur, rev, transfers)?;
    let pairs = pairs(cur, rev, by_position);
    let comments = crate::comment_transfer::Comparison::new(cur, rev, &pairs)?;
    let cur_matched: HashSet<usize> = pairs.iter().map(|p| p.0).collect();
    let rev_matched: HashSet<usize> = pairs.iter().map(|p| p.1).collect();
    let mut out = vec![];
    for (i, j) in &pairs {
        let (Some(a), Some(b)) = (cur.slides.get(*i), rev.slides.get(*j)) else { continue };
        let mut details = slide_details(cur, a, rev, b, &comments)?;
        if !templates.same_selection(a.presentation_template, b.presentation_template) {
            details.push(if a.presentation_template == b.presentation_template {
                "presentation template resource".into()
            } else {
                "presentation template selection".into()
            });
        }
        if !details.is_empty() {
            out.push(Change { kind: ChangeKind::Changed, slide: *i, revised: Some(*j), details });
        }
    }
    for (i, s) in cur.slides.iter().enumerate() {
        if !cur_matched.contains(&i) {
            out.push(Change { kind: ChangeKind::Deleted, slide: i, revised: None, details: vec![slide_title(s)] });
        }
    }
    let matched_position: HashMap<_, _> = pairs.iter().map(|(current, revised)| (*revised, *current + 1)).collect();
    let mut at = 0;
    for (j, s) in rev.slides.iter().enumerate() {
        if let Some(position) = matched_position.get(&j) {
            at = *position;
        }
        if !rev_matched.contains(&j) {
            // After the open slide that matches the revised slide before it.
            out.push(Change { kind: ChangeKind::Inserted, slide: at, revised: Some(j), details: vec![slide_title(s)] });
        }
    }
    out.sort_by_key(|c| (c.slide, c.revised.unwrap_or(0)));
    Ok(out)
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

const MAX_REVIEW_SHAPES: usize = 100_000;

/// Actual matched owners, rather than coincident IDs or names, establish which
/// attachment references mean the same thing after a shape-ID transfer.
struct AttachmentOwners {
    current: HashMap<ShapeId, Option<usize>>,
    revised: HashMap<ShapeId, Option<usize>>,
    pairs: HashMap<usize, usize>,
}

impl AttachmentOwners {
    fn new(current: &[Shape], revised: &[Shape], matched: &[Option<usize>]) -> Result<Self> {
        fn index(shapes: &[Shape], out: &mut HashMap<ShapeId, Option<usize>>, depth: usize, count: &mut usize) -> Result<()> {
            *count = count
                .checked_add(shapes.len())
                .filter(|count| *count <= MAX_REVIEW_SHAPES)
                .ok_or_else(|| invalid("shape owner count exceeds limit"))?;
            if depth > 64 {
                return Err(invalid("shape owner nesting exceeds limit"));
            }
            for shape in shapes {
                // Duplicate IDs cannot identify one actual target owner.
                out.entry(shape.id).and_modify(|owner| *owner = None).or_insert(Some(shape as *const Shape as usize));
                if !shape.children().is_empty() {
                    index(shape.children(), out, depth + 1, count)?;
                }
            }
            Ok(())
        }
        fn pair(current: &Shape, revised: &Shape, out: &mut HashMap<usize, usize>, depth: usize, count: &mut usize) -> Result<()> {
            *count = count.checked_add(1).filter(|count| *count <= MAX_REVIEW_SHAPES).ok_or_else(|| invalid("shape pair count exceeds limit"))?;
            if depth > 64 {
                return Err(invalid("shape pair nesting exceeds limit"));
            }
            out.insert(current as *const Shape as usize, revised as *const Shape as usize);
            // plain_shape compares group children in their stored order. Use
            // exactly those positional child pairs, including nested groups.
            for (current, revised) in current.children().iter().zip(revised.children()) {
                pair(current, revised, out, depth + 1, count)?;
            }
            Ok(())
        }
        let mut owners = Self { current: HashMap::new(), revised: HashMap::new(), pairs: HashMap::new() };
        let mut count = 0;
        index(current, &mut owners.current, 0, &mut count)?;
        count = 0;
        index(revised, &mut owners.revised, 0, &mut count)?;
        count = 0;
        for (current, revised_index) in current.iter().zip(matched) {
            if let Some(revised) = revised_index.and_then(|index| revised.get(index)) {
                pair(current, revised, &mut owners.pairs, 0, &mut count)?;
            }
        }
        Ok(owners)
    }

    fn same_end(&self, current: Option<(ShapeId, u32)>, revised: Option<(ShapeId, u32)>) -> bool {
        match (current, revised) {
            (None, None) => true,
            (Some((current, current_site)), Some((revised, revised_site))) if current_site == revised_site => {
                match (self.current.get(&current), self.revised.get(&revised)) {
                    (Some(Some(current)), Some(Some(revised))) => self.pairs.get(current) == Some(revised),
                    // A missing reference remains its exact missing ID. Resolved
                    // but unmatched/ambiguous targets never compare as equivalent.
                    (None, None) => current == revised,
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn align_for_comparison(&self, current: &mut Shape, revised: &Shape) -> bool {
        if let (ShapeKind::Connector { start, end }, ShapeKind::Connector { start: other_start, end: other_end }) = (&mut current.kind, &revised.kind)
        {
            if !self.same_end(*start, *other_start) || !self.same_end(*end, *other_end) {
                return false;
            }
            // Translate IDs only in a comparison copy, after proving the actual
            // target pair and site. Neither endpoint is erased or guessed.
            *start = *other_start;
            *end = *other_end;
        }
        match (current.children_mut(), revised.children()) {
            (Some(current), revised) => {
                current.len() == revised.len()
                    && current.iter_mut().zip(revised).all(|(current, revised)| self.align_for_comparison(current, revised))
            }
            (None, revised) => revised.is_empty(),
        }
    }
}

/// Retain the legacy top-level ID-then-unused-name pairing rule, but share its
/// exact result with attachment comparison and bound adversarial search work.
fn shape_pairs(current: &[Shape], revised: &[Shape]) -> Result<Vec<Option<usize>>> {
    if current.len() > MAX_REVIEW_SHAPES || revised.len() > MAX_REVIEW_SHAPES {
        return Err(invalid("shape count exceeds limit"));
    }
    let mut matched = Vec::new();
    let mut used = HashSet::new();
    let mut work = 0usize;
    let mut charge = |bytes: usize| -> Result<()> {
        work = work
            .checked_add(bytes)
            .filter(|work| *work <= presentation_template::MAX_TEMPLATE_SCAN_WORK)
            .ok_or_else(|| invalid("shape matching work exceeds limit"))?;
        Ok(())
    };
    for current in current {
        let mut found = None;
        for (index, revised) in revised.iter().enumerate() {
            charge(1)?;
            if current.id == revised.id {
                found = Some(index);
                break;
            }
        }
        if found.is_none() && !current.name.is_empty() {
            for (index, revised) in revised.iter().enumerate() {
                charge(1)?;
                if !used.contains(&index) && !revised.name.is_empty() {
                    charge(current.name.len().min(revised.name.len()).saturating_add(1))?;
                    if current.name == revised.name {
                        found = Some(index);
                        break;
                    }
                }
            }
        }
        matched.push(found.filter(|index| used.insert(*index)));
    }
    Ok(matched)
}

fn shape_details(cp: &Presentation, a: &Shape, rp: &Presentation, b: &Shape, owners: &AttachmentOwners, out: &mut Vec<String>) {
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
        if !owners.align_for_comparison(&mut x, &y) || x != y {
            what.push("properties");
        }
    }
    if !what.is_empty() {
        out.push(format!("{label}: {}", what.join(", ")));
    }
}

fn slide_details(
    cp: &Presentation,
    a: &Slide,
    rp: &Presentation,
    b: &Slide,
    comments: &crate::comment_transfer::Comparison<'_>,
) -> Result<Vec<String>> {
    let mut out = vec![];
    let matched = shape_pairs(&a.shapes, &b.shapes)?;
    let owners = AttachmentOwners::new(&a.shapes, &b.shapes, &matched)?;
    let mut used: HashSet<usize> = HashSet::new();
    for (sa, k) in a.shapes.iter().zip(matched) {
        match k {
            Some(k) if !used.contains(&k) => {
                used.insert(k);
                if let Some(sb) = b.shapes.get(k) {
                    shape_details(cp, sa, rp, sb, &owners, &mut out);
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
    if !crate::comment_transfer::equivalent(&a.comments, &b.comments, comments, |a, b| owners.same_end(Some((a, 0)), Some((b, 0))))? {
        out.push("comments".into());
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
    Ok(out)
}

fn max_shape_id(shapes: &[Shape], m: &mut u32) {
    deckcraft_model::walk(shapes, &mut |s, _| *m = (*m).max(s.id.0));
}

/// The revised slide made to live in `doc`: its media copied over (remapped on an id clash), shape
/// ids kept unless another slide of `doc` uses them (then fresh, animations following), and its
/// layout matched by id, then by name, then the first layout. An error when `doc` has used every
/// object id and cannot take the slide without duplicating ids (#407).
pub fn adopt(doc: &mut Presentation, rev: &Presentation, slide: &Slide, replacing: Option<SlideId>) -> crate::Result<Slide> {
    crate::transfer_policy::check_external(rev, std::iter::once(slide))?;
    notes_review_boundary(doc, rev)?;
    let closure = template_transfer::capture(rev, std::iter::once(slide))?;
    deckcraft_model::comments::validate(rev, true).map_err(|e| invalid(&e.to_string()))?;
    let mut candidate = doc.clone();
    deckcraft_model::comments::normalize(&mut candidate).map_err(|e| invalid(&e.to_string()))?;
    candidate.next_id = candidate.next_id.max(rev.next_id);
    let mut slide = slide.clone();
    crate::comment_transfer::capture(rev, std::slice::from_mut(&mut slide))?;
    let mut slide_map = review_slide_map(&candidate, rev);
    let id = review_target(&mut candidate, slide.id, replacing, &mut HashSet::new())?;
    slide_map.insert(slide.id, id);
    template_transfer::remap(&mut candidate, &closure, std::slice::from_mut(&mut slide))?;
    let slide = adopt_body(&mut candidate, rev, slide, Some(id), &slide_map, &mut crate::comment_transfer::Context::default())?;
    presentation_template::validate(&candidate)?;
    *doc = candidate;
    Ok(slide)
}

fn review_slide_map(doc: &Presentation, rev: &Presentation) -> BTreeMap<SlideId, SlideId> {
    pairs(doc, rev, unrelated(doc, rev)).into_iter().filter_map(|(a, b)| Some((rev.slides.get(b)?.id, doc.slides.get(a)?.id))).collect()
}
fn review_target(doc: &mut Presentation, source: SlideId, replacing: Option<SlideId>, reserved: &mut HashSet<SlideId>) -> Result<SlideId> {
    let id = if let Some(id) = replacing {
        id
    } else if source.0 != 0 && !doc.slides.iter().any(|s| s.id == source) && !reserved.contains(&source) {
        source
    } else {
        SlideId(doc.alloc_id().ok_or_else(|| crate::cmd::bad("review.accept", crate::cmd::NO_ID_SPACE))?)
    };
    reserved.insert(id);
    doc.next_id = doc.next_id.max(id.0);
    Ok(id)
}
fn adopt_body(
    doc: &mut Presentation,
    rev: &Presentation,
    mut sl: Slide,
    replacing: Option<SlideId>,
    comment_slides: &BTreeMap<SlideId, SlideId>,
    comment_context: &mut crate::comment_transfer::Context,
) -> crate::Result<Slide> {
    let comment_shapes = sl.shapes.clone();
    let mut comments = std::mem::take(&mut sl.comments);
    let mut mids = crate::media_refs::slide_media(&sl);
    deckcraft_model::comments::resources::visit::comments(&comments, &mut |id| mids.push(*id));
    mids.sort_unstable();
    mids.dedup();
    let mut media_map: HashMap<MediaId, MediaId> = HashMap::new();
    for id in mids {
        let m = rev.media(id).ok_or_else(|| invalid("referenced review media is missing"))?;
        match doc.media(id) {
            Some(x) if x.data == m.data && x.content_type == m.content_type && x.link == m.link => {}
            None => doc.media.push(m.clone()),
            Some(_) => {
                let new = doc
                    .add_media(&m.name, &m.content_type, m.data.as_ref().clone())
                    .ok_or_else(|| crate::cmd::bad("review.accept", crate::cmd::NO_ID_SPACE))?;
                media_map.insert(id, new);
            }
        }
    }
    crate::media_refs::visit_mut::slide(&mut sl, &mut |id| {
        if let Some(new) = media_map.get(id) {
            *id = *new;
        }
    });
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
        depth: usize,
        count: &mut usize,
    ) -> crate::Result<()> {
        if depth > 64 {
            return Err(invalid("adopted shape nesting exceeds limit"));
        }
        *count = count
            .checked_add(shapes.len())
            .filter(|count| *count <= MAX_REVIEW_SHAPES)
            .ok_or_else(|| invalid("adopted shape count exceeds limit"))?;
        for s in shapes {
            if taken.contains(&s.id.0) {
                let new = ShapeId(doc.alloc_id().ok_or_else(|| crate::cmd::bad("review.accept", crate::cmd::NO_ID_SPACE))?);
                ids.insert(s.id, new);
                s.id = new;
            }
            if let Some(ch) = s.children_mut() {
                rec(doc, ch, taken, ids, depth + 1, count)?;
            }
        }
        Ok(())
    }
    let mut count = 0;
    rec(doc, &mut sl.shapes, &taken, &mut id_map, 0, &mut count)?;
    // References can precede their targets, or cross group boundaries. Wait
    // until every target has its final ID, then visit all connectors once.
    fn attachments(shapes: &mut [Shape], ids: &HashMap<ShapeId, ShapeId>, depth: usize, count: &mut usize) -> Result<()> {
        if depth > 64 {
            return Err(invalid("adopted attachment nesting exceeds limit"));
        }
        *count = count
            .checked_add(shapes.len())
            .filter(|count| *count <= MAX_REVIEW_SHAPES)
            .ok_or_else(|| invalid("adopted attachment count exceeds limit"))?;
        for shape in shapes {
            if let ShapeKind::Connector { start, end } = &mut shape.kind {
                for (id, _) in start.iter_mut().chain(end.iter_mut()) {
                    if let Some(new) = ids.get(id) {
                        *id = *new;
                    }
                }
            }
            if let Some(children) = shape.children_mut() {
                attachments(children, ids, depth + 1, count)?;
            }
        }
        Ok(())
    }
    count = 0;
    attachments(&mut sl.shapes, &id_map, 0, &mut count)?;
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
    let shape_map = crate::comment_transfer::identity_shapes(&comment_shapes, &id_map);
    let comment_media = crate::comment_transfer::identity_media(&comments, &media_map);
    crate::comment_transfer::remap(doc, &mut comments, &shape_map, &comment_media, comment_slides, comment_context)?;
    sl.comments = comments;
    // Layout.
    if doc.layout(sl.layout).is_none() {
        let name = rev.layout(sl.layout).map(|(_, l)| l.name.clone());
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
    if !doc.presentation_templates.is_empty() || !rev.presentation_templates.is_empty() {
        return Err(invalid("template-bearing review acceptance needs the session's ownership receipt; use review.compare and review.accept"));
    }
    let outcome = accept_changes(doc, rev, std::slice::from_ref(change), &mut ReviewTransfers::default())?;
    Ok(outcome.map(|(_, at)| at))
}

/// Accept one fixed change set. Resources are transferred once for the whole set,
/// so shared A/A/B ownership stays shared and a remapped selection cannot make
/// accept-all revisit the same page indefinitely.
pub(crate) fn accept_changes(
    doc: &mut Presentation,
    rev: &Presentation,
    changes: &[Change],
    transfers: &mut ReviewTransfers,
) -> Result<Option<(usize, usize)>> {
    notes_review_boundary(doc, rev)?;
    deckcraft_model::comments::validate(doc, true).map_err(|e| invalid(&e.to_string()))?;
    deckcraft_model::comments::validate(rev, true).map_err(|e| invalid(&e.to_string()))?;
    let limit = doc.slides.len().checked_add(rev.slides.len()).ok_or_else(|| invalid("change count overflow"))?;
    if changes.len() > limit {
        return Err(invalid("too many review changes"));
    }
    crate::transfer_policy::check_external(
        rev,
        changes.iter().filter_map(|change| change.revised.and_then(|index| rev.slides.get(index).map(Arc::as_ref))),
    )?;
    TemplateState::new(doc, rev, transfers)?;
    let mut plans = Vec::new();
    let mut sources = Vec::new();
    let mut deletions = Vec::new();
    for change in changes {
        match change.kind {
            ChangeKind::Changed | ChangeKind::Inserted => {
                if change.kind == ChangeKind::Changed && doc.slides.get(change.slide).is_none() {
                    continue;
                }
                let Some(source) = change.revised.and_then(|index| rev.slides.get(index)) else { continue };
                plans.push(change.clone());
                sources.push(source.as_ref().clone());
            }
            ChangeKind::Deleted => {
                if let Some(slide) = doc.slides.get(change.slide) {
                    deletions.push((change.slide, slide.id));
                }
            }
        }
    }
    if plans.is_empty() && deletions.is_empty() {
        return Ok(None);
    }
    crate::comment_transfer::capture(rev, &mut sources)?;
    let closure = template_transfer::capture(rev, sources.iter())?;
    let mut candidate = doc.clone();
    deckcraft_model::comments::normalize(&mut candidate).map_err(|e| invalid(&e.to_string()))?;
    candidate.next_id = candidate.next_id.max(rev.next_id);
    let mut comment_slides = review_slide_map(&candidate, rev);
    let mut reserved = HashSet::new();
    let mut targets = Vec::new();
    for (plan, source) in plans.iter().zip(&sources) {
        let replacing = if plan.kind == ChangeKind::Changed { candidate.slides.get(plan.slide).map(|s| s.id) } else { None };
        let target = review_target(&mut candidate, source.id, replacing, &mut reserved)?;
        targets.push(target);
        comment_slides.insert(source.id, target);
    }
    let mut comment_context = crate::comment_transfer::Context::default();
    let mut staged = transfers.clone();
    let live: HashMap<_, _> = doc.presentation_templates.iter().map(|owner| (owner.id, owner)).collect();
    let mut retired: HashSet<_> = staged.retired.iter().map(|owner| Arc::as_ptr(owner) as usize).collect();
    for index in
        plans.iter().filter(|change| change.kind == ChangeKind::Changed).map(|change| change.slide).chain(deletions.iter().map(|(index, _)| *index))
    {
        if let Some(owner) = doc.slides.get(index).and_then(|slide| slide.presentation_template).and_then(|id| live.get(&id))
            && retired.insert(Arc::as_ptr(owner) as usize)
        {
            staged.retired.push((*owner).clone());
        }
    }
    template_transfer::remap_with(&mut candidate, &closure, &mut sources, &mut staged.receipt)?;
    let mut proofs: HashSet<_> =
        staged.accepted.iter().map(|(source, destination)| (Arc::as_ptr(source) as usize, Arc::as_ptr(destination) as usize)).collect();
    for (source, destination) in staged.receipt.pairs() {
        if proofs.insert((Arc::as_ptr(source) as usize, Arc::as_ptr(destination) as usize)) {
            staged.accepted.push((source.clone(), destination.clone()));
        }
    }
    staged.validate_history()?;
    let mut accepted = 0usize;
    let mut focus = None;
    for (index, plan) in plans.iter().enumerate().filter(|(_, plan)| plan.kind == ChangeKind::Changed) {
        let Some(id) = candidate.slides.get(plan.slide).map(|slide| slide.id) else { continue };
        let source = sources.get_mut(index).ok_or_else(|| invalid("changed source disappeared"))?;
        let source = std::mem::take(source);
        let slide = adopt_body(&mut candidate, rev, source, Some(id), &comment_slides, &mut comment_context)?;
        let target = candidate.slides.get_mut(plan.slide).ok_or_else(|| invalid("changed page owner disappeared"))?;
        *target = Arc::new(slide);
        focus = Some(id);
        accepted += 1;
    }
    // Insert at original boundaries in reverse order, before deleting original
    // pages. Same-boundary inserts then retain their revised source ordering.
    let mut inserts: Vec<_> = plans.iter().enumerate().filter(|(_, plan)| plan.kind == ChangeKind::Inserted).map(|(index, _)| index).collect();
    inserts.sort_unstable_by_key(|index| plans.get(*index).map(|plan| (plan.slide, plan.revised)));
    for index in inserts.into_iter().rev() {
        let plan = plans.get(index).ok_or_else(|| invalid("insert plan disappeared"))?;
        let source = sources.get_mut(index).ok_or_else(|| invalid("insert source disappeared"))?;
        let source = std::mem::take(source);
        let id = *targets.get(index).ok_or_else(|| invalid("insert target disappeared"))?;
        let slide = adopt_body(&mut candidate, rev, source, Some(id), &comment_slides, &mut comment_context)?;
        let at = plan.slide.min(candidate.slides.len());
        focus = Some(slide.id);
        candidate.slides.insert(at, Arc::new(slide));
        accepted += 1;
    }
    let allow = candidate.slides.len().saturating_sub(1);
    let removed: HashSet<_> = deletions.iter().take(allow).map(|(_, id)| *id).collect();
    accepted = accepted.saturating_add(removed.len());
    candidate.slides.retain(|slide| !removed.contains(&slide.id));
    if accepted == 0 {
        return Ok(None);
    }
    presentation_template::validate(&candidate)?;
    let at = focus
        .and_then(|id| candidate.slide_index(id))
        .unwrap_or_else(|| deletions.last().map(|(index, _)| *index).unwrap_or(0).min(candidate.slides.len().saturating_sub(1)));
    *doc = candidate;
    *transfers = staged;
    Ok(Some((accepted, at)))
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
