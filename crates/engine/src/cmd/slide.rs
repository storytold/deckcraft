//! Slides: new, duplicate, delete, move, layout, reset, hide, notes, sections, navigation.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_model::text::TextBody;
use deckcraft_model::{LayoutId, LayoutType, Presentation, Section, SlideId, defaults};
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, bool_or, bool_param, cmd, has_doc, has_slide, ok, opt_usize_param, str_param, usize_param};
use crate::{Result, Session, Target};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "slide.new",
            "New Slide",
            ["Home", "Slides"],
            Some("Cmd+Shift+N"),
            "{layout?: layout name or kind (title, titleAndContent, sectionHeader, twoContent, comparison, titleOnly, blank, contentWithCaption, pictureWithCaption), at?: index, title?: text, body?: text}",
            has_doc,
            new
        ),
        cmd!("slide.duplicate", "Duplicate Slide", ["Insert"], None, "{index?}", has_slide, duplicate),
        cmd!("slide.delete", "Delete Slide", ["Edit"], None, "{index?, ids?: [slide id]}", has_slide, delete),
        cmd!(
            "slide.move",
            "Move Slide",
            [],
            None,
            "{from?: index, ids?: [slide id], to: index (counted without the moved slides)}",
            has_slide,
            move_slide
        ),
        cmd!(noundo "slide.go", "Go to Slide", [], None, "{index} | {id}", has_slide, go),
        cmd!(noundo "slide.next", "Next Slide", [], Some("PageDown"), "{}", has_slide, next),
        cmd!(noundo "slide.previous", "Previous Slide", [], Some("PageUp"), "{}", has_slide, prev),
        cmd!(noundo "slide.first", "First Slide", [], Some("Home"), "{}", has_slide, first),
        cmd!(noundo "slide.last", "Last Slide", [], Some("End"), "{}", has_slide, last),
        cmd!(noundo "slide.selectRange", "Select Slide Range", [], None, "{to: index, from?: index (default the current slide), add?: bool (keep the slides already selected)}", has_slide, select_range),
        cmd!(noundo "slide.selectSlides", "Select Slides", [], None, "{ids?: [slide id], indices?: [index], add?: bool}", has_slide, select_slides),
        cmd!("slide.hide", "Hide Slide", ["Slide Show", "Set Up"], None, "{index?, hidden?: bool}", has_slide, hide),
        cmd!("slide.layout", "Layout", ["Home", "Slides"], None, "{layout: name or kind, index?}", has_slide, set_layout),
        cmd!("slide.reset", "Reset", ["Home", "Slides"], None, "{index?}", has_slide, reset),
        cmd!(
            "slide.notes",
            "Notes",
            ["View"],
            None,
            "{text | body: formatted notes (as in slide.inspect's notesBody), index?} (text as typed: untouched notes keep their formatting)",
            has_slide,
            notes
        ),
        cmd!("slide.rename", "Rename Slide", [], None, "{name, index?}", has_slide, rename),
        cmd!("section.add", "Add Section", ["Home", "Slides", "Section"], None, "{name?, at?: slide index}", has_slide, section_add),
        cmd!("section.rename", "Rename Section", ["Home", "Slides", "Section"], None, "{index, name}", has_slide, section_rename),
        cmd!(
            "section.remove",
            "Remove Section",
            ["Home", "Slides", "Section"],
            None,
            "{index, slides?: bool (also delete its slides)}",
            has_slide,
            section_remove
        ),
        cmd!("section.removeAll", "Remove All Sections", ["Home", "Slides", "Section"], None, "{}", has_slide, section_remove_all),
        cmd!("section.move", "Move Section", [], None, "{index, to}", has_slide, section_move),
        cmd!(
            "slide.fromOutline",
            "Slides from Outline…",
            ["Insert", "Slides From"],
            None,
            "{text, markdown?: bool (headings are slides, list items bullets)}",
            has_doc,
            from_outline
        ),
    ]
}

/// Resolve a layout by id, name or kind keyword.
pub fn find_layout(doc: &Presentation, key: &Value, master: usize) -> Option<LayoutId> {
    if let Some(n) = key.as_u64() {
        let id = LayoutId(u32::try_from(n).ok()?);
        if doc.layout(id).is_some() {
            return Some(id);
        }
    }
    let s = key.as_str()?;
    let m = doc.masters.get(master).or(doc.masters.first())?;
    let norm = |x: &str| x.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let k = norm(s);
    let kind = match k.as_str() {
        "title" | "titleslide" => Some(LayoutType::Title),
        "titleandcontent" | "content" | "obj" => Some(LayoutType::TitleAndContent),
        "sectionheader" | "section" => Some(LayoutType::SectionHeader),
        "twocontent" | "two" => Some(LayoutType::TwoContent),
        "comparison" => Some(LayoutType::Comparison),
        "titleonly" => Some(LayoutType::TitleOnly),
        "blank" => Some(LayoutType::Blank),
        "contentwithcaption" => Some(LayoutType::ContentWithCaption),
        "picturewithcaption" | "picture" => Some(LayoutType::PictureWithCaption),
        "titleandverticaltext" => Some(LayoutType::TitleAndVerticalText),
        "verticaltitleandtext" => Some(LayoutType::VerticalTitleAndText),
        _ => None,
    };
    m.layouts.iter().find(|l| norm(&l.name) == k).or_else(|| kind.and_then(|kd| m.layouts.iter().find(|l| l.kind == kd))).map(|l| l.id)
}

fn current_master(doc: &Presentation, sel: &crate::Selection) -> usize {
    doc.slides.get(sel.slide).and_then(|s| doc.masters.iter().position(|m| m.layout(s.layout).is_some())).unwrap_or(0)
}

fn new(s: &mut Session, p: &Value) -> Result<Value> {
    s.edit(|doc, sel| {
        let master = current_master(doc, sel);
        let layout = match p.get("layout") {
            Some(k) => find_layout(doc, k, master).ok_or_else(|| bad("slide.new", format!("no layout {k}")))?,
            None => {
                // PowerPoint picks Title and Content after a title slide, else the current layout.
                let cur = doc.slides.get(sel.slide).map(|x| x.layout);
                let is_title = cur.and_then(|l| doc.layout(l)).is_some_and(|(_, l)| l.kind == LayoutType::Title);
                match cur {
                    Some(l) if !is_title => l,
                    _ => defaults::layout_of_kind(doc, LayoutType::TitleAndContent).or(cur).unwrap_or_default(),
                }
            }
        };
        let mut slide = defaults::new_slide(doc, layout).ok_or_else(|| bad("slide.new", NO_ID_SPACE))?;
        if let Some(t) = str_param(p, "title")
            && let Some(sh) = slide.shapes.iter_mut().find(|x| x.ph_type().is_some_and(|k| k.is_title()))
        {
            sh.text = Some(TextBody::from_text(t));
        }
        if let Some(b) = str_param(p, "body")
            && let Some(sh) = body_placeholder(&mut slide.shapes)
        {
            let mut body = TextBody::from_text(b);
            // Leading tabs set the outline level, as in text.set.
            for para in &mut body.paragraphs {
                super::text::apply_leading_tabs(para);
            }
            sh.text = Some(body);
        }
        let at = usize_param(p, "at").unwrap_or(if doc.slides.is_empty() { 0 } else { sel.slide + 1 }).min(doc.slides.len());
        let id = slide.id;
        // Inherit the section of the slide before.
        let section = at.checked_sub(1).and_then(|i| doc.slides.get(i)).and_then(|prev| doc.section_of(prev.id));
        doc.slides.insert(at, Arc::new(slide));
        if let Some(si) = section
            && let Some(sec) = doc.sections.get_mut(si)
        {
            let pos = at
                .checked_sub(1)
                .and_then(|i| doc.slides.get(i))
                .and_then(|prev| sec.slides.iter().position(|x| *x == prev.id))
                .map(|k| k + 1)
                .unwrap_or(sec.slides.len());
            sec.slides.insert(pos.min(sec.slides.len()), id);
        } else if let Some(first) = doc.sections.first_mut() {
            first.slides.insert(0, id);
        }
        sel.slide = at;
        sel.slides = vec![id];
        sel.shapes.clear();
        sel.text = None;
        sel.target = Target::Slides;
        Ok(json!({"index": at, "id": id}))
    })
}

/// The placeholder a new slide's `body` goes into: the layout's main content placeholder.
/// Prefers `idx="1"` (the content slot PowerPoint fills), then a full-size text placeholder over
/// a `half`/`quarter` one, then a text placeholder over a picture/chart/table one; ties keep
/// shape order, so a layout with a single body placeholder behaves as before.
fn body_placeholder(shapes: &mut [deckcraft_model::Shape]) -> Option<&mut deckcraft_model::Shape> {
    use deckcraft_model::PhType;
    let rank = |sh: &deckcraft_model::Shape| {
        let ph = sh.ph.as_ref()?;
        if ph.kind.is_title() {
            return None;
        }
        let text = matches!(ph.kind, PhType::Body | PhType::Obj | PhType::SubTitle);
        let small = matches!(ph.size.as_deref(), Some("half" | "quarter"));
        Some((ph.idx != 1, small, !text))
    };
    let best = shapes.iter().enumerate().filter_map(|(i, sh)| rank(sh).map(|r| (r, i))).min()?.1;
    shapes.get_mut(best)
}

fn index_of(s: &Session, p: &Value) -> Result<usize> {
    let st = s.doc()?;
    let i = opt_usize_param(p, "index", "slide")?.unwrap_or(st.selection.slide);
    if i >= st.doc.slides.len() {
        return Err(bad("slide", format!("no slide {i}")));
    }
    Ok(i)
}

fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let indices: Vec<usize> = match opt_usize_param(p, "index", "slide.duplicate")? {
        Some(i) => vec![i],
        None => {
            let st = s.doc()?;
            let mut v: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
            if v.is_empty() {
                v.push(st.selection.slide);
            }
            v.sort_unstable();
            v
        }
    };
    s.edit(|doc, sel| {
        let last = *indices.last().unwrap_or(&0);
        let mut new_ids = vec![];
        for (k, i) in indices.iter().enumerate() {
            let Some(src) = doc.slides.get(*i).map(|x| x.as_ref().clone()) else { continue };
            let mut copy = src;
            copy.id = SlideId(doc.alloc_id().ok_or_else(|| bad("slide.duplicate", NO_ID_SPACE))?);
            // Animations follow the new shape ids, triggers included.
            let map = super::edit::reid("slide.duplicate", doc, &mut copy.shapes)?;
            super::edit::remap_animations(&mut copy.animations, &map);
            let section = doc.section_of(src_id(doc, *i));
            let at = (last + 1 + k).min(doc.slides.len());
            new_ids.push(copy.id);
            let nid = copy.id;
            doc.slides.insert(at, Arc::new(copy));
            if let Some(si) = section
                && let Some(sec) = doc.sections.get_mut(si)
            {
                sec.slides.push(nid);
                // Keep the section list in document order so the copy sits next to its source.
                let order: Vec<SlideId> = doc.slides.iter().map(|x| x.id).collect();
                sec.slides.sort_by_key(|x| order.iter().position(|o| o == x).unwrap_or(usize::MAX));
            }
        }
        sel.slide = (last + new_ids.len()).min(doc.slides.len().saturating_sub(1));
        sel.slides = new_ids.clone();
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"ids": new_ids}))
    })
}

fn src_id(doc: &Presentation, i: usize) -> SlideId {
    doc.slides.get(i).map(|s| s.id).unwrap_or_default()
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let ids: Vec<SlideId> = {
        let st = s.doc()?;
        if let Some(a) = p.get("ids").and_then(Value::as_array) {
            a.iter().filter_map(Value::as_u64).filter_map(|v| u32::try_from(v).ok()).map(SlideId).collect()
        } else if let Some(i) = opt_usize_param(p, "index", "slide.delete")? {
            st.doc.slides.get(i).map(|x| vec![x.id]).unwrap_or_default()
        } else if !st.selection.slides.is_empty() {
            st.selection.slides.clone()
        } else {
            st.current_slide().map(|x| vec![x.id]).unwrap_or_default()
        }
    };
    if ids.is_empty() {
        return Err(bad("slide.delete", "no such slide"));
    }
    s.edit(|doc, sel| {
        let first = ids.iter().filter_map(|id| doc.slide_index(*id)).min().unwrap_or(0);
        doc.slides.retain(|x| !ids.contains(&x.id));
        for sec in &mut doc.sections {
            sec.slides.retain(|x| !ids.contains(x));
        }
        for show in &mut doc.custom_shows {
            show.slides.retain(|x| !ids.contains(x));
        }
        sel.slide = first.min(doc.slides.len().saturating_sub(1));
        sel.slides = doc.slides.get(sel.slide).map(|x| vec![x.id]).unwrap_or_default();
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"deleted": ids.len(), "remaining": doc.slides.len()}))
    })
}

/// Moves one slide (`from`, else the current slide) or several (`ids`) to `to`, in one undo step.
/// `to` counts in the deck without the moved slides; they land there as one run, in the order
/// they had, join the section of the slide before them, and stay selected.
fn move_slide(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = p.get("ids").filter(|v| !v.is_null());
    let from = match ids {
        None => opt_usize_param(p, "from", "slide.move")?.unwrap_or(s.doc()?.selection.slide),
        Some(_) => 0,
    };
    let to = usize_param(p, "to").ok_or_else(|| bad("slide.move", "missing `to`"))?;
    s.edit(|doc, sel| {
        let moving: HashSet<SlideId> = match ids {
            Some(v) => slide_ids_param(v, doc, "slide.move")?,
            None => {
                let id = doc.slides.get(from).map(|x| x.id).ok_or_else(|| bad("slide.move", format!("no slide {from}")))?;
                HashSet::from([id])
            }
        };
        let (block, mut rest): (Vec<_>, Vec<_>) = std::mem::take(&mut doc.slides).into_iter().partition(|x| moving.contains(&x.id));
        let to = to.min(rest.len());
        let tail = rest.split_off(to);
        let moved: Vec<SlideId> = block.iter().map(|x| x.id).collect();
        rest.extend(block);
        rest.extend(tail);
        doc.slides = rest;
        reassign_sections(doc, &moved, to);
        sel.slide = to;
        sel.slides = moved;
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"index": to, "moved": moving.len()}))
    })
}

/// The slides an `ids` param names: a non-empty list of ids of slides in `doc` (repeats fold).
/// The first id that is not a slide of `doc` is an error, so the set never outgrows the deck.
fn slide_ids_param(v: &Value, doc: &Presentation, cmd: &str) -> Result<HashSet<SlideId>> {
    let a = v.as_array().ok_or_else(|| bad(cmd, "`ids` must be a list of slide ids"))?;
    let known: HashSet<SlideId> = doc.slides.iter().map(|x| x.id).collect();
    let mut out = HashSet::new();
    for x in a {
        let id = x.as_u64().and_then(|n| u32::try_from(n).ok()).map(SlideId).filter(|id| known.contains(id));
        out.insert(id.ok_or_else(|| bad(cmd, format!("no slide with id {x}")))?);
    }
    if out.is_empty() {
        return Err(bad(cmd, "`ids` is empty"));
    }
    Ok(out)
}

/// Keeps sections in step after `moved` landed as a run at `at`: the run joins the section of
/// the slide just before it (the first section when it opens the deck), in document order.
fn reassign_sections(doc: &mut Presentation, moved: &[SlideId], at: usize) {
    if doc.sections.is_empty() {
        return;
    }
    let moving: HashSet<SlideId> = moved.iter().copied().collect();
    for sec in &mut doc.sections {
        sec.slides.retain(|x| !moving.contains(x));
    }
    let neighbour = at.checked_sub(1).and_then(|i| doc.slides.get(i)).map(|x| x.id);
    let si = neighbour.and_then(|n| doc.section_of(n)).unwrap_or(0);
    let order: HashMap<SlideId, usize> = doc.slides.iter().enumerate().map(|(i, x)| (x.id, i)).collect();
    if let Some(sec) = doc.sections.get_mut(si) {
        sec.slides.extend_from_slice(moved);
        sec.slides.sort_by_key(|x| order.get(x).copied().unwrap_or(usize::MAX));
    }
}

fn go_to(s: &mut Session, i: usize) -> Result<Value> {
    s.select(|doc, sel| {
        let n = doc.slides.len();
        sel.slide = i.min(n.saturating_sub(1));
        sel.slides = doc.slides.get(sel.slide).map(|x| vec![x.id]).unwrap_or_default();
        sel.shapes.clear();
        sel.text = None;
        sel.cells = None;
        sel.range_anchor = None;
        sel.target = Target::Slides;
    })?;
    Ok(json!({"index": s.doc()?.selection.slide}))
}

fn go(s: &mut Session, p: &Value) -> Result<Value> {
    let i = match (usize_param(p, "index"), p.get("id").and_then(Value::as_u64)) {
        (Some(i), _) => i,
        (None, Some(id)) => s
            .doc()?
            .doc
            .slide_index(SlideId(u32::try_from(id).map_err(|_| bad("slide.go", "no such slide"))?))
            .ok_or_else(|| bad("slide.go", "no such slide"))?,
        _ => return Err(bad("slide.go", "missing `index`")),
    };
    if i >= s.doc()?.doc.slides.len() {
        return Err(bad("slide.go", format!("no slide {i}")));
    }
    go_to(s, i)
}
fn next(s: &mut Session, _p: &Value) -> Result<Value> {
    let i = s.doc()?.selection.slide + 1;
    go_to(s, i)
}
fn prev(s: &mut Session, _p: &Value) -> Result<Value> {
    let i = s.doc()?.selection.slide.saturating_sub(1);
    go_to(s, i)
}
fn first(s: &mut Session, _p: &Value) -> Result<Value> {
    go_to(s, 0)
}
fn last(s: &mut Session, _p: &Value) -> Result<Value> {
    let n = s.doc()?.doc.slides.len();
    go_to(s, n.saturating_sub(1))
}

fn select_slides(s: &mut Session, p: &Value) -> Result<Value> {
    let add = bool_or(p, "add", false);
    let ids: Vec<SlideId> = {
        let st = s.doc()?;
        let mut v: Vec<SlideId> = p
            .get("ids")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_u64).filter_map(|x| u32::try_from(x).ok()).map(SlideId).collect())
            .unwrap_or_default();
        if let Some(a) = p.get("indices").and_then(Value::as_array) {
            v.extend(a.iter().filter_map(Value::as_u64).filter_map(|i| st.doc.slides.get(i as usize)).map(|x| x.id));
        }
        v
    };
    s.select(|doc, sel| {
        if add {
            let mut have: std::collections::HashSet<_> = sel.slides.iter().copied().collect();
            for i in &ids {
                if have.insert(*i) {
                    sel.slides.push(*i);
                }
            }
        } else {
            sel.slides = ids.clone();
        }
        if let Some(last) = ids.last().and_then(|i| doc.slide_index(*i)) {
            sel.slide = last;
        }
        sel.shapes.clear();
        sel.text = None;
        sel.range_anchor = None;
    })?;
    Ok(json!({"selected": s.doc()?.selection.slides}))
}

/// Shift-click in the slide list: select every slide from `from` to `to`. Without `from` the range
/// starts at the anchor of the previous Shift-click (so a second Shift-click extends from the same
/// slide, as in PowerPoint), else at the current slide.
/// With `add` the slides already selected stay selected (Ctrl+Shift-click).
fn select_range(s: &mut Session, p: &Value) -> Result<Value> {
    let to = usize_param(p, "to").ok_or_else(|| bad("slide.selectRange", "missing `to`"))?;
    let given = opt_usize_param(p, "from", "slide.selectRange")?;
    let add = bool_or(p, "add", false);
    let (from, count) = {
        let st = s.doc()?;
        let sel = &st.selection;
        let current = st.doc.slides.get(sel.slide).map(|x| x.id);
        let anchor = sel.range_anchor.filter(|(_, end)| Some(*end) == current).and_then(|(a, _)| st.doc.slide_index(a));
        (given.or(anchor).unwrap_or(sel.slide), st.doc.slides.len())
    };
    if to >= count || from >= count {
        return Err(bad("slide.selectRange", "no such slide"));
    }
    let (a, b) = (from.min(to), from.max(to));
    s.select(|doc, sel| {
        let range: Vec<SlideId> = doc.slides.iter().skip(a).take(b - a + 1).map(|x| x.id).collect();
        if add {
            for id in range {
                if !sel.slides.contains(&id) {
                    sel.slides.push(id);
                }
            }
        } else {
            sel.slides = range;
        }
        sel.slide = to;
        sel.range_anchor = doc.slides.get(from).zip(doc.slides.get(to)).map(|(a, b)| (a.id, b.id));
        sel.shapes.clear();
        sel.text = None;
        sel.cells = None;
        sel.target = Target::Slides;
    })?;
    Ok(json!({"selected": s.doc()?.selection.slides}))
}

fn hide(s: &mut Session, p: &Value) -> Result<Value> {
    let targets: Vec<usize> = match opt_usize_param(p, "index", "slide.hide")? {
        Some(i) => vec![i],
        None => {
            let st = s.doc()?;
            let v: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
            if v.is_empty() { vec![st.selection.slide] } else { v }
        }
    };
    s.edit(|doc, _| {
        let all_hidden = targets.iter().all(|i| doc.slides.get(*i).is_some_and(|x| x.hidden));
        let value = bool_param(p, "hidden").unwrap_or(!all_hidden);
        for i in &targets {
            if let Some(sl) = doc.slides.get_mut(*i) {
                Arc::make_mut(sl).hidden = value;
            }
        }
        Ok(json!({"hidden": value}))
    })
}

/// Where PowerPoint puts a placeholder the new layout has no counterpart for: against the side edge
/// of the slide that its centre is nearer, keeping its top, width and height (observed on slides
/// 960 and 1100 pt wide, with content placeholders and with a footer and a slide number: every one
/// went to `0` or to `width - w`). One whose centre is exactly the middle went to the left edge.
fn nearest_side(x: deckcraft_model::Xfrm, width: f64) -> deckcraft_model::Xfrm {
    if !(width.is_finite() && width > 0.0 && x.x.is_finite() && x.w.is_finite()) {
        return x;
    }
    let left = x.x + x.w / 2.0 <= width / 2.0 + 0.01;
    deckcraft_model::Xfrm { x: if left { 0.0 } else { width - x.w }, ..x }
}

/// Re-map a slide's placeholders onto a new layout (content kept, matched by type/index).
pub fn apply_layout(doc: &mut Presentation, index: usize, layout: LayoutId) -> Result<()> {
    apply_layout_with(doc, index, layout, true)
}

/// [`apply_layout`] for a change of theme: the placeholders the new layout has no counterpart for
/// stay as they are drawn, with the idx they had. (PowerPoint copies the slide's layout into the
/// new design instead, and nothing is left over; that is not done here.)
pub fn apply_layout_in_place(doc: &mut Presentation, index: usize, layout: LayoutId) -> Result<()> {
    apply_layout_with(doc, index, layout, false)
}

fn apply_layout_with(doc: &mut Presentation, index: usize, layout: LayoutId, leftovers_to_the_edges: bool) -> Result<()> {
    let lay_shapes: Vec<deckcraft_model::Shape> =
        doc.layout(layout).map(|(_, l)| l.shapes.clone()).ok_or_else(|| bad("slide.layout", "no such layout"))?;
    let mut fresh = vec![];
    for ls in lay_shapes.iter().filter(|x| x.ph.as_ref().is_some_and(|p| !p.kind.is_footer_kind())) {
        let id = deckcraft_model::ShapeId(doc.alloc_id().ok_or_else(|| bad("slide.layout", NO_ID_SPACE))?);
        fresh.push((id, ls.clone()));
    }
    // A placeholder that inherits its frame keeps it where it is drawn now: one left over below
    // pairs with nothing in the new layout, and would lose its frame and vanish.
    let frames: Vec<Option<deckcraft_model::Xfrm>> = {
        let slide = doc.slides.get(index).ok_or_else(|| bad("slide.layout", "no slide"))?;
        let ctx = deckcraft_model::resolve::Ctx::for_slide(doc, slide);
        // A footer, date or slide number pairs with the new layout's by idx, and then takes its frame.
        let pairs_anyway = |s: &deckcraft_model::Shape| {
            s.ph.as_ref().is_some_and(|p| p.kind.is_footer_kind() && lay_shapes.iter().any(|l| l.ph.as_ref().is_some_and(|q| q.idx == p.idx)))
        };
        let inherits = |s: &deckcraft_model::Shape| s.ph.is_some() && s.xfrm.is_none() && !pairs_anyway(s);
        slide
            .shapes
            .iter()
            .map(|s| {
                ctx.as_ref().filter(|c| inherits(s) && !deckcraft_model::resolve::lacks_frame(c, s)).map(|c| deckcraft_model::resolve::xfrm(c, s))
            })
            .collect()
    };
    let width = doc.slide_size.width;
    let slide = doc.slides.get_mut(index).ok_or_else(|| bad("slide.layout", "no slide"))?;
    let slide = Arc::make_mut(slide);
    let mut old: Vec<deckcraft_model::Shape> = std::mem::take(&mut slide.shapes);
    for (o, f) in old.iter_mut().zip(frames) {
        if f.is_some() {
            o.xfrm = f;
        }
    }
    let mut out = vec![];
    for (id, ls) in fresh {
        let Some(lph) = ls.ph.clone() else { continue };
        // Find matching old placeholder: same idx, else same title-ness/compatible type.
        let k = old
            .iter()
            .position(|o| o.ph.as_ref().is_some_and(|p| p.kind == lph.kind && (p.idx == lph.idx || lph.kind.is_title())))
            .or_else(|| old.iter().position(|o| o.ph.as_ref().is_some_and(|p| p.kind.is_title() == lph.kind.is_title() && !p.kind.is_footer_kind())));
        match k {
            Some(k) => {
                let mut o = old.remove(k);
                if let Some(p) = o.ph.as_mut() {
                    p.kind = lph.kind;
                    p.idx = lph.idx;
                }
                o.xfrm = None;
                out.push(o);
            }
            None => {
                out.push(deckcraft_model::Shape { id, name: ls.name.clone(), ph: Some(lph), text: Some(TextBody::default()), ..Default::default() })
            }
        }
    }
    // Leftover placeholders with content stay (empty ones go). As in PowerPoint, one with a frame of
    // its own becomes an orphan (it pairs with nothing, on this layout or the next), a footer, date
    // or slide number the new layout lacks included, and is moved against the side edge of the
    // slide nearer its centre.
    for mut o in old {
        let empty_ph = o.ph.is_some() && o.text.as_ref().is_none_or(|t| t.is_empty()) && matches!(o.kind, deckcraft_model::ShapeKind::Shape);
        if empty_ph {
            continue;
        }
        let footer_pairs =
            o.ph.as_ref().is_some_and(|p| p.kind.is_footer_kind() && lay_shapes.iter().any(|l| l.ph.as_ref().is_some_and(|q| q.idx == p.idx)));
        if footer_pairs {
            // A footer, date or slide number the new layout has too takes its frame.
            o.xfrm = None;
        } else if leftovers_to_the_edges && let (Some(p), Some(x)) = (o.ph.as_mut(), o.xfrm) {
            p.idx = deckcraft_model::resolve::ORPHAN_IDX;
            o.xfrm = Some(nearest_side(x, width));
        }
        out.push(o);
    }
    slide.shapes = out;
    slide.layout = layout;
    Ok(())
}

fn set_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let key = p.get("layout").cloned().ok_or_else(|| bad("slide.layout", "missing `layout`"))?;
    let targets: Vec<usize> = match opt_usize_param(p, "index", "slide.layout")? {
        Some(i) => vec![i],
        None => {
            let st = s.doc()?;
            let v: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
            if v.is_empty() { vec![st.selection.slide] } else { v }
        }
    };
    s.edit(|doc, sel| {
        let master = current_master(doc, sel);
        let layout = find_layout(doc, &key, master).ok_or_else(|| bad("slide.layout", format!("no layout {key}")))?;
        for i in &targets {
            apply_layout(doc, *i, layout)?;
        }
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"layout": layout}))
    })
}

fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    let i = index_of(s, p)?;
    s.edit(|doc, sel| {
        let layout = doc.slides.get(i).map(|x| x.layout).unwrap_or_default();
        apply_layout(doc, i, layout)?;
        // Reset puts a placeholder back on its layout's frame. One that pairs with nothing in the
        // layout has none to go back to and keeps its own, as in PowerPoint.
        let keeps_frame: Vec<bool> = doc
            .slides
            .get(i)
            .and_then(|sl| {
                let ctx = deckcraft_model::resolve::Ctx::for_slide(doc, sl)?;
                let orphan = |s: &deckcraft_model::Shape| s.ph.as_ref().is_some_and(|p| p.idx == deckcraft_model::resolve::ORPHAN_IDX);
                Some(sl.shapes.iter().map(|s| orphan(s) || deckcraft_model::resolve::unpaired_placeholder(&ctx, s)).collect())
            })
            .unwrap_or_default();
        if let Some(sl) = doc.slides.get_mut(i) {
            let sl = Arc::make_mut(sl);
            sl.background = None;
            for (k, sh) in sl.shapes.iter_mut().enumerate() {
                if sh.ph.is_some() {
                    if !keeps_frame.get(k).copied().unwrap_or(false) {
                        sh.xfrm = None;
                    }
                    sh.fill = None;
                    sh.line = None;
                    sh.effects = None;
                    sh.geom = Default::default();
                    if let Some(t) = sh.text.as_mut() {
                        t.body = Default::default();
                        t.list_style = Default::default();
                        for para in &mut t.paragraphs {
                            para.props = Default::default();
                            for r in &mut para.runs {
                                r.props = Default::default();
                            }
                        }
                    }
                }
            }
        }
        sel.shapes.clear();
        Ok(())
    })?;
    ok()
}

fn notes(s: &mut Session, p: &Value) -> Result<Value> {
    let i = index_of(s, p)?;
    // Formatted notes as a whole (the notes editors), or text: then only what was retyped
    // changes, so bold, levels and bullets around it survive.
    let body = match p.get("body") {
        Some(b) => Some(serde_json::from_value::<TextBody>(b.clone()).map_err(|e| bad("slide.notes", format!("invalid `body`: {e}")))?),
        None => None,
    };
    let text = str_param(p, "text").unwrap_or("").to_string();
    s.edit(|doc, _| {
        if let Some(sl) = doc.slides.get_mut(i) {
            let notes = &mut Arc::make_mut(sl).notes;
            match body {
                Some(b) => *notes = b,
                None => deckcraft_model::edit::replace_text(notes, &text),
            }
        }
        Ok(())
    })?;
    ok()
}

fn rename(s: &mut Session, p: &Value) -> Result<Value> {
    let i = index_of(s, p)?;
    let name = str_param(p, "name").unwrap_or("").to_string();
    s.edit(|doc, _| {
        if let Some(sl) = doc.slides.get_mut(i) {
            Arc::make_mut(sl).name = name;
        }
        Ok(())
    })?;
    ok()
}

fn section_add(s: &mut Session, p: &Value) -> Result<Value> {
    let at = opt_usize_param(p, "at", "section.add")?.unwrap_or(s.doc()?.selection.slide);
    let name = str_param(p, "name").unwrap_or("Untitled Section").to_string();
    s.edit(|doc, _| {
        let at = at.min(doc.slides.len().saturating_sub(1));
        let ids: Vec<SlideId> = doc.slides.iter().map(|x| x.id).collect();
        if doc.sections.is_empty() {
            // The slides before `at` go into a "Default Section".
            if at > 0 {
                doc.sections.push(Section { name: "Default Section".into(), slides: ids.get(..at).map(<[_]>::to_vec).unwrap_or_default() });
            }
            doc.sections.push(Section { name, slides: ids.get(at..).map(<[_]>::to_vec).unwrap_or_default() });
        } else {
            let Some(start) = ids.get(at).copied() else { return Err(bad("section.add", "no slide")) };
            let si = doc.section_of(start).unwrap_or(0);
            let sec = doc.sections.get_mut(si).ok_or_else(|| bad("section.add", "no section"))?;
            let k = sec.slides.iter().position(|x| *x == start).unwrap_or(sec.slides.len());
            let tail = sec.slides.split_off(k);
            doc.sections.insert(si + 1, Section { name, slides: tail });
            doc.sections.retain(|x| !x.slides.is_empty() || x.name != "Default Section");
        }
        Ok(json!({"sections": doc.sections.len()}))
    })
}

fn section_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").ok_or_else(|| bad("section.rename", "missing `index`"))?;
    let name = str_param(p, "name").unwrap_or("").to_string();
    s.edit(|doc, _| {
        let sec = doc.sections.get_mut(i).ok_or_else(|| bad("section.rename", "no such section"))?;
        sec.name = name;
        Ok(())
    })?;
    ok()
}

fn section_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").ok_or_else(|| bad("section.remove", "missing `index`"))?;
    let with_slides = bool_or(p, "slides", false);
    s.edit(|doc, sel| {
        if i >= doc.sections.len() {
            return Err(bad("section.remove", "no such section"));
        }
        let sec = doc.sections.remove(i);
        if with_slides {
            doc.slides.retain(|x| !sec.slides.contains(&x.id));
            sel.slide = sel.slide.min(doc.slides.len().saturating_sub(1));
        } else if let Some(prev) = i.checked_sub(1).and_then(|k| doc.sections.get_mut(k)) {
            prev.slides.extend(sec.slides);
        } else if let Some(next) = doc.sections.get_mut(0) {
            let mut v = sec.slides;
            v.append(&mut next.slides);
            next.slides = v;
        }
        Ok(())
    })?;
    ok()
}

fn section_remove_all(s: &mut Session, _p: &Value) -> Result<Value> {
    s.edit(|doc, _| {
        doc.sections.clear();
        Ok(())
    })?;
    ok()
}

fn section_move(s: &mut Session, p: &Value) -> Result<Value> {
    let i = usize_param(p, "index").ok_or_else(|| bad("section.move", "missing `index`"))?;
    let to = usize_param(p, "to").ok_or_else(|| bad("section.move", "missing `to`"))?;
    s.edit(|doc, _| {
        if i >= doc.sections.len() {
            return Err(bad("section.move", "no such section"));
        }
        let sec = doc.sections.remove(i);
        let to = to.min(doc.sections.len());
        doc.sections.insert(to, sec);
        // Reorder slides to follow sections.
        let order: Vec<SlideId> = doc.sections.iter().flat_map(|x| x.slides.iter().copied()).collect();
        doc.slides.sort_by_key(|x| order.iter().position(|o| *o == x.id).unwrap_or(usize::MAX));
        Ok(())
    })?;
    ok()
}

fn from_outline(s: &mut Session, p: &Value) -> Result<Value> {
    let text = str_param(p, "text").unwrap_or("").to_string();
    let markdown = bool_param(p, "markdown").unwrap_or(false);
    s.edit(|doc, sel| {
        let before = doc.slides.len();
        let n = if markdown { deckcraft_format::markdown_to_slides(doc, &text) } else { deckcraft_format::outline_to_slides(doc, &text) };
        // both append; move them after the current slide.
        let at = (sel.slide + 1).min(before);
        let new: Vec<_> = doc.slides.drain(before..).collect();
        for (k, sl) in new.into_iter().enumerate() {
            doc.slides.insert(at + k, sl);
        }
        // Join the section of the slide before, as slide.new does.
        let ids: Vec<SlideId> = doc.slides.iter().skip(at).take(n).map(|x| x.id).collect();
        let prev = at.checked_sub(1).and_then(|i| doc.slides.get(i)).map(|x| x.id);
        let section = prev.and_then(|p| doc.section_of(p));
        if let Some(sec) = section.and_then(|si| doc.sections.get_mut(si)) {
            let pos = prev.and_then(|p| sec.slides.iter().position(|x| *x == p)).map(|k| k + 1).unwrap_or(sec.slides.len());
            let pos = pos.min(sec.slides.len());
            sec.slides.splice(pos..pos, ids);
        } else if let Some(first) = doc.sections.first_mut() {
            first.slides.splice(0..0, ids);
        }
        Ok(json!({"slides": n}))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Session;

    #[test]
    fn invalid_index_params_are_rejected_not_defaulted_to_selection() {
        let mut s = Session::with_new();
        while s.doc().expect("doc").doc.slides.len() < 3 {
            s.execute("slide.new", &json!({})).expect("new slide");
        }
        for bad in [json!(-1), json!(1.5)] {
            for (cmd, p) in [
                ("slide.delete", json!({"index": bad})),
                ("slide.duplicate", json!({"index": bad})),
                ("slide.move", json!({"from": bad, "to": 0})),
                ("section.add", json!({"at": bad})),
            ] {
                assert!(s.execute(cmd, &p).is_err(), "{cmd} must reject {p}");
                assert_eq!(s.doc().expect("doc").doc.slides.len(), 3, "{cmd} {p}");
            }
        }
    }

    /// A deck of `n` slides and its slide ids, in order.
    fn deck(n: usize) -> (Session, Vec<u32>) {
        let mut s = Session::with_new();
        while s.doc().expect("doc").doc.slides.len() < n {
            s.execute("slide.new", &json!({})).expect("new slide");
        }
        let ids = order(&s);
        (s, ids)
    }

    fn order(s: &Session) -> Vec<u32> {
        s.doc().expect("doc").doc.slides.iter().map(|x| x.id.0).collect()
    }

    fn selected(s: &Session) -> (usize, Vec<u32>) {
        let sel = &s.doc().expect("doc").selection;
        (sel.slide, sel.slides.iter().map(|x| x.0).collect())
    }

    #[test]
    fn deleting_slide_prunes_every_custom_show_in_one_undo_step() {
        let (mut s, ids) = deck(3);
        let shows = vec![
            deckcraft_model::CustomShow {
                name: "First".into(),
                slides: vec![deckcraft_model::SlideId(ids[0]), deckcraft_model::SlideId(ids[1]), deckcraft_model::SlideId(ids[1])],
            },
            deckcraft_model::CustomShow { name: "Second".into(), slides: vec![deckcraft_model::SlideId(ids[1]), deckcraft_model::SlideId(ids[2])] },
            deckcraft_model::CustomShow { name: "Empty after delete".into(), slides: vec![deckcraft_model::SlideId(ids[1])] },
        ];
        s.edit(|doc, _| {
            doc.custom_shows = shows;
            Ok(())
        })
        .expect("set custom shows");

        s.execute("slide.delete", &json!({"index": 1})).expect("delete slide");
        let shows = &s.doc().expect("doc").doc.custom_shows;
        assert_eq!(shows[0].slides, [deckcraft_model::SlideId(ids[0])]);
        assert_eq!(shows[1].slides, [deckcraft_model::SlideId(ids[2])]);
        assert!(shows[2].slides.is_empty());

        s.execute("edit.undo", &json!({})).expect("undo deletion");
        assert_eq!(order(&s), ids);
        assert_eq!(
            s.doc().expect("doc").doc.custom_shows[0].slides,
            [deckcraft_model::SlideId(ids[0]), deckcraft_model::SlideId(ids[1]), deckcraft_model::SlideId(ids[1])]
        );
        assert_eq!(s.doc().expect("doc").doc.custom_shows[1].slides, [deckcraft_model::SlideId(ids[1]), deckcraft_model::SlideId(ids[2])]);

        s.execute("edit.redo", &json!({})).expect("redo deletion");
        let bytes = deckcraft_format::save(&s.doc().expect("doc").doc).expect("save deck");
        let reopened = deckcraft_format::load(&bytes).expect("reopen deck");
        assert_eq!(reopened.custom_shows[0].slides, [deckcraft_model::SlideId(ids[0])]);
        assert_eq!(reopened.custom_shows[1].slides, [deckcraft_model::SlideId(ids[2])]);
        assert!(reopened.custom_shows[2].slides.is_empty());
    }

    #[test]
    fn move_from_to_moves_one_slide() {
        let (mut s, ids) = deck(4);
        s.execute("slide.move", &json!({"from": 3, "to": 0})).expect("move");
        assert_eq!(order(&s), [ids[3], ids[0], ids[1], ids[2]]);
        assert_eq!(selected(&s), (0, vec![ids[3]]));
    }

    #[test]
    fn move_ids_lands_them_as_one_run_in_deck_order() {
        let (mut s, ids) = deck(6);
        // Listed backwards: they still land in the order they had.
        s.execute("slide.move", &json!({"ids": [ids[4], ids[1]], "to": 0})).expect("to the front");
        assert_eq!(order(&s), [ids[1], ids[4], ids[0], ids[2], ids[3], ids[5]]);
        assert_eq!(selected(&s), (0, vec![ids[1], ids[4]]), "the moved slides stay selected");

        let (mut s, ids) = deck(6);
        s.execute("slide.move", &json!({"ids": [ids[0], ids[2]], "to": 2})).expect("to the middle");
        assert_eq!(order(&s), [ids[1], ids[3], ids[0], ids[2], ids[4], ids[5]]);
        assert_eq!(selected(&s), (2, vec![ids[0], ids[2]]));

        let (mut s, ids) = deck(6);
        s.execute("slide.move", &json!({"ids": [ids[1], ids[3]], "to": 1000})).expect("past the end clamps");
        assert_eq!(order(&s), [ids[0], ids[2], ids[4], ids[5], ids[1], ids[3]]);
        assert_eq!(selected(&s), (4, vec![ids[1], ids[3]]));
    }

    #[test]
    fn moving_several_slides_is_one_undo_step() {
        let (mut s, ids) = deck(5);
        s.execute("slide.selectSlides", &json!({"ids": [ids[0], ids[3]]})).expect("select");
        let before = selected(&s);
        s.execute("slide.move", &json!({"ids": [ids[0], ids[3]], "to": 3})).expect("move");
        let after = order(&s);
        assert_eq!(after, [ids[1], ids[2], ids[4], ids[0], ids[3]]);
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(order(&s), ids);
        assert_eq!(selected(&s), before);
        s.execute("edit.redo", &json!({})).expect("redo");
        assert_eq!(order(&s), after);
    }

    #[test]
    fn moved_slides_join_the_section_before_them() {
        let (mut s, ids) = deck(5);
        s.execute("section.add", &json!({"name": "A", "at": 0})).expect("A");
        s.execute("section.add", &json!({"name": "B", "at": 3})).expect("B");
        // A = 0,1,2; B = 3,4. Move 0 and 1 to the end: B gets them, A keeps only slide 2.
        s.execute("slide.move", &json!({"ids": [ids[0], ids[1]], "to": 3})).expect("move");
        let sections =
            |s: &Session| -> Vec<Vec<u32>> { s.doc().expect("doc").doc.sections.iter().map(|x| x.slides.iter().map(|id| id.0).collect()).collect() };
        assert_eq!(sections(&s), [vec![ids[2]], vec![ids[3], ids[4], ids[0], ids[1]]]);
        // Move the whole of A away: A stays, empty; the run at the front joins the first section.
        s.execute("slide.move", &json!({"ids": [ids[2]], "to": 4})).expect("empty A");
        assert_eq!(sections(&s), [vec![], vec![ids[3], ids[4], ids[0], ids[1], ids[2]]]);
        s.execute("slide.move", &json!({"ids": [ids[0], ids[1]], "to": 0})).expect("to the front");
        assert_eq!(sections(&s), [vec![ids[0], ids[1]], vec![ids[3], ids[4], ids[2]]]);
        let all: Vec<u32> = sections(&s).concat();
        assert_eq!(all, order(&s), "sections follow slide order");
    }

    #[test]
    fn bad_ids_are_rejected_and_change_nothing() {
        let (mut s, ids) = deck(3);
        let lots_of_unknown: Vec<u64> = (1_000_000..1_100_000).collect();
        for p in [
            json!({"ids": [], "to": 0}),
            json!({"ids": [ids[0], 999_999], "to": 0}),
            json!({"ids": [-1], "to": 0}),
            json!({"ids": ["1"], "to": 0}),
            json!({"ids": [1.5], "to": 0}),
            json!({"ids": [u64::MAX], "to": 0}),
            json!({"ids": ids[0], "to": 0}),
            json!({"ids": lots_of_unknown, "to": 0}),
            json!({"ids": [ids[1]]}),
        ] {
            assert!(s.execute("slide.move", &p).is_err(), "slide.move must reject {p}");
            assert_eq!(order(&s), ids, "{p}");
        }
        // Repeats fold into one.
        let repeated = vec![ids[2]; 100_000];
        s.execute("slide.move", &json!({"ids": repeated, "to": 0})).expect("repeats");
        assert_eq!(order(&s), [ids[2], ids[0], ids[1]]);
        assert_eq!(selected(&s), (0, vec![ids[2]]));
    }

    #[test]
    fn duplicate_keeps_section_order_in_step_with_slides() {
        let mut s = Session::with_new();
        while s.doc().expect("doc").doc.slides.len() < 3 {
            s.execute("slide.new", &json!({})).expect("new slide");
        }
        s.execute("section.add", &json!({"at": 0})).expect("section");
        let orig = s.doc().expect("doc").doc.slides.first().expect("slide").id;
        s.execute("slide.duplicate", &json!({"index": 0})).expect("dup");
        let st = s.doc().expect("doc");
        let all: Vec<_> = st.doc.slides.iter().map(|x| x.id).collect();
        let sec = &st.doc.sections.first().expect("section").slides;
        assert_eq!(all.len(), 4);
        assert_eq!(sec, &all, "section order must match slide order");
        assert_eq!(sec.first(), Some(&orig));
    }

    #[test]
    fn new_slide_body_leading_tabs_become_levels() {
        let mut s = Session::with_new();
        let text = "Level one\n\tLevel two\n\t\tLevel three\nLevel one again";
        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Indent", "body": text})).expect("new slide");
        let st = s.doc().expect("doc");
        let slide = st.doc.slides.get(st.selection.slide).expect("slide");
        let body = slide.shapes.iter().find(|x| x.ph_type().is_some_and(|k| !k.is_title())).expect("body");
        let paras = &body.text.as_ref().expect("text").paragraphs;
        let got: Vec<(u8, String)> = paras.iter().map(|p| (p.level, p.text())).collect();
        let want = [(0, "Level one"), (1, "Level two"), (2, "Level three"), (0, "Level one again")];
        assert_eq!(got, want.iter().map(|(l, t)| (*l, t.to_string())).collect::<Vec<_>>());
    }

    fn deck_of(n: usize) -> Session {
        let mut s = Session::with_new();
        while s.doc().expect("doc").doc.slides.len() < n {
            s.execute("slide.new", &json!({})).expect("new slide");
        }
        s
    }

    fn selected_indices(s: &Session) -> Vec<usize> {
        let st = s.doc().expect("doc");
        let mut v: Vec<usize> = st.selection.slides.iter().filter_map(|id| st.doc.slide_index(*id)).collect();
        v.sort_unstable();
        v
    }

    #[test]
    fn select_range_covers_every_slide_between() {
        let mut s = deck_of(6);
        s.execute("slide.go", &json!({"index": 1})).expect("go");
        s.execute("slide.selectRange", &json!({"to": 4})).expect("range");
        assert_eq!(selected_indices(&s), vec![1, 2, 3, 4]);
        assert_eq!(s.doc().expect("doc").selection.slide, 4);
        // Backwards works too, and replaces the earlier selection.
        s.execute("slide.selectRange", &json!({"from": 5, "to": 3})).expect("range");
        assert_eq!(selected_indices(&s), vec![3, 4, 5]);
    }

    #[test]
    fn select_range_add_keeps_the_existing_selection() {
        let mut s = deck_of(8);
        s.execute("slide.selectSlides", &json!({"indices": [0, 1]})).expect("select");
        s.execute("slide.go", &json!({"index": 4})).expect("go");
        s.execute("slide.selectSlides", &json!({"indices": [4]})).expect("select");
        s.execute("slide.selectRange", &json!({"to": 6, "add": true})).expect("range");
        assert_eq!(selected_indices(&s), vec![4, 5, 6]);
        s.execute("slide.selectSlides", &json!({"indices": [0]})).expect("select");
        s.execute("slide.selectRange", &json!({"from": 3, "to": 5, "add": true})).expect("range");
        assert_eq!(selected_indices(&s), vec![0, 3, 4, 5]);
    }

    #[test]
    fn select_range_rejects_out_of_range_slides() {
        let mut s = deck_of(3);
        assert!(s.execute("slide.selectRange", &json!({"to": 3})).is_err());
        assert!(s.execute("slide.selectRange", &json!({"from": 9, "to": 1})).is_err());
        assert!(s.execute("slide.selectRange", &json!({})).is_err());
    }

    #[test]
    fn select_range_rejects_an_invalid_from_instead_of_using_the_current_slide() {
        let mut s = deck_of(4);
        s.execute("slide.go", &json!({"index": 2})).expect("go");
        assert!(s.execute("slide.selectRange", &json!({"from": -1, "to": 3})).is_err());
        assert!(s.execute("slide.selectRange", &json!({"from": "x", "to": 3})).is_err());
        assert_eq!(selected_indices(&s), vec![2]);
    }

    #[test]
    fn a_second_range_extends_from_the_original_anchor() {
        let mut s = deck_of(8);
        s.execute("slide.go", &json!({"index": 2})).expect("go");
        s.execute("slide.selectRange", &json!({"to": 5})).expect("range");
        assert_eq!(selected_indices(&s), vec![2, 3, 4, 5]);
        // Shift-click further on: still anchored at 2, not at 5.
        s.execute("slide.selectRange", &json!({"to": 7})).expect("range");
        assert_eq!(selected_indices(&s), vec![2, 3, 4, 5, 6, 7]);
        // Shift-click back across the anchor.
        s.execute("slide.selectRange", &json!({"to": 0})).expect("range");
        assert_eq!(selected_indices(&s), vec![0, 1, 2]);
        // A plain click on another slide starts a new anchor.
        s.execute("slide.go", &json!({"index": 4})).expect("go");
        s.execute("slide.selectRange", &json!({"to": 6})).expect("range");
        assert_eq!(selected_indices(&s), vec![4, 5, 6]);
    }

    #[test]
    fn moving_a_slide_drops_the_range_anchor() {
        let mut s = deck_of(6);
        s.execute("slide.go", &json!({"index": 0})).expect("go");
        s.execute("slide.selectRange", &json!({"to": 2})).expect("range");
        // Slide 0 moves to index 2 and stays current; index 0 now holds another slide, so the
        // next Shift-click starts at the moved slide instead of at index 0.
        s.execute("slide.move", &json!({"from": 0, "to": 2})).expect("move");
        assert_eq!(s.doc().expect("doc").selection.slide, 2);
        s.execute("slide.selectRange", &json!({"to": 4})).expect("range");
        assert_eq!(selected_indices(&s), vec![2, 3, 4]);
    }

    #[test]
    fn select_range_clears_a_table_cell_selection_and_returns_to_the_slides() {
        let mut s = deck_of(3);
        s.select(|_, sel| {
            sel.cells = Some((deckcraft_model::ShapeId(1), (0, 0), (1, 1)));
            sel.target = crate::Target::Master { master: 0 };
        })
        .expect("cells");
        s.execute("slide.selectRange", &json!({"to": 2})).expect("range");
        let sel = &s.doc().expect("doc").selection;
        assert!(sel.cells.is_none());
        assert_eq!(sel.target, crate::Target::Slides);
    }

    // ---- a placeholder the new layout has no counterpart for (observed in PowerPoint) ----

    use std::sync::Arc;

    use deckcraft_model::resolve::ORPHAN_IDX;
    use deckcraft_model::text::TextBody;
    use deckcraft_model::{Layout, LayoutId, PhType, Placeholder, Presentation, Shape, ShapeId, Xfrm};

    fn frame(x: f64, w: f64) -> Xfrm {
        Xfrm::new(x, 150.0, w, 320.0)
    }

    /// A layout with a title and one content placeholder per `(idx, frame)`, in the first master.
    fn add_layout(p: &mut Presentation, name: &str, contents: &[(u32, Xfrm)]) -> LayoutId {
        let id = LayoutId(p.alloc_id().expect("layout id"));
        let mut shapes = vec![Shape {
            id: ShapeId(p.alloc_id().expect("title id")),
            ph: Some(Placeholder { kind: PhType::Title, idx: 0, ..Default::default() }),
            ..Default::default()
        }];
        for (idx, xfrm) in contents {
            let ph = Some(Placeholder { kind: PhType::Obj, idx: *idx, ..Default::default() });
            shapes.push(Shape { id: ShapeId(p.alloc_id().expect("content id")), ph, xfrm: Some(*xfrm), ..Default::default() });
        }
        if let Some(m) = p.masters.first_mut() {
            Arc::make_mut(m).layouts.push(Layout { id, name: name.into(), shapes, show_master_shapes: true, ..Default::default() });
        }
        id
    }

    struct Case {
        width: f64,
        b: Xfrm,
        c: Xfrm,
        target: Vec<(u32, Xfrm)>,
        /// Frames set by hand on the slide for "B" and "C", rather than inherited.
        explicit: Option<(Xfrm, Xfrm)>,
    }

    /// A deck with layout SRC (title, B at idx 10, C at idx 11), a slide "A", "B", "C" on it and
    /// layout DST; the slide is then moved to DST.
    fn move_to_dst(case: &Case) -> Session {
        let mut s = seeded(case);
        s.execute("slide.layout", &json!({"layout": "DST"})).expect("layout");
        s
    }

    /// The deck of [`move_to_dst`] before the slide is moved.
    fn seeded(case: &Case) -> Session {
        let mut s = Session::with_new();
        s.edit(|doc, _| {
            doc.slide_size.width = case.width;
            let src = add_layout(doc, "SRC", &[(10, case.b), (11, case.c)]);
            add_layout(doc, "DST", &case.target);
            let mut shapes = vec![];
            for (text, idx, kind) in [("A", 0, PhType::Title), ("B", 10, PhType::Obj), ("C", 11, PhType::Obj)] {
                let id = ShapeId(doc.alloc_id().expect("shape id"));
                let ph = Some(Placeholder { kind, idx, ..Default::default() });
                let xfrm = case.explicit.and_then(|(b, c)| match text {
                    "B" => Some(b),
                    "C" => Some(c),
                    _ => None,
                });
                shapes.push(Shape { id, ph, xfrm, text: Some(TextBody::from_text(text)), ..Default::default() });
            }
            if let Some(slide) = doc.slides.first_mut().map(Arc::make_mut) {
                slide.layout = src;
                slide.shapes = shapes;
            }
            Ok(())
        })
        .expect("seed");
        s
    }

    fn shape_with(s: &Session, text: &str) -> Shape {
        let st = s.doc().expect("doc");
        st.doc.slides[0].shapes.iter().find(|x| x.text.as_ref().is_some_and(|t| t.text() == text)).cloned().expect("the shape")
    }

    fn left_of(s: &Session, text: &str) -> Option<f64> {
        shape_with(s, text).xfrm.map(|x| x.x)
    }

    #[test]
    fn a_placeholder_without_a_counterpart_becomes_an_orphan_against_the_nearer_side_edge() {
        let full = vec![(10, frame(60.0, 840.0))];
        // (case, expected left edge of "C"): every one of these was observed in PowerPoint.
        let cases = [
            (Case { width: 960.0, b: frame(60.0, 300.0), c: frame(520.0, 200.0), target: full.clone(), explicit: None }, 760.0),
            (Case { width: 960.0, b: frame(60.0, 300.0), c: frame(395.0, 405.0), target: full.clone(), explicit: None }, 555.0),
            (
                Case {
                    width: 960.0,
                    b: frame(60.0, 405.0),
                    c: frame(495.0, 405.0),
                    target: vec![(10, Xfrm::new(100.0, 100.0, 400.0, 300.0))],
                    explicit: None,
                },
                555.0,
            ),
            (Case { width: 1100.0, b: frame(60.0, 405.0), c: frame(495.0, 405.0), target: full.clone(), explicit: None }, 695.0),
            // Frames set by hand: "C" (centre left of the middle) goes to the left edge, and "B" takes the layout's frame.
            (
                Case {
                    width: 960.0,
                    b: frame(60.0, 405.0),
                    c: frame(495.0, 405.0),
                    target: full.clone(),
                    explicit: Some((Xfrm::new(200.0, 200.0, 300.0, 150.0), Xfrm::new(100.0, 100.0, 300.0, 200.0))),
                },
                0.0,
            ),
        ];
        for (case, left) in cases {
            let s = move_to_dst(&case);
            let c = shape_with(&s, "C");
            assert_eq!(c.ph.as_ref().map(|p| p.idx), Some(ORPHAN_IDX), "C pairs with nothing now");
            let x = c.xfrm.expect("C has a frame of its own");
            assert_eq!(x.x, left, "C of {:?}", case.c);
            // The top, width and height are kept.
            let kept = case.explicit.map_or(case.c, |(_, c)| c);
            assert_eq!((x.y, x.w, x.h), (kept.y, kept.w, kept.h));
            // "B" pairs with the layout's idx 10 and takes whatever that gives, not its own frame.
            let b = shape_with(&s, "B");
            assert_eq!((b.ph.as_ref().map(|p| p.idx), b.xfrm), (Some(10), None));
        }
    }

    #[test]
    fn with_no_content_placeholder_in_the_new_layout_both_become_orphans_on_their_own_sides() {
        let case = Case { width: 960.0, b: frame(60.0, 405.0), c: frame(495.0, 405.0), target: vec![], explicit: None };
        let s = move_to_dst(&case);
        assert_eq!((left_of(&s, "B"), left_of(&s, "C")), (Some(0.0), Some(555.0)));
    }

    #[test]
    fn going_back_to_the_first_layout_pairs_the_orphans_again() {
        let case = Case { width: 960.0, b: frame(60.0, 405.0), c: frame(495.0, 405.0), target: vec![(10, frame(60.0, 840.0))], explicit: None };
        let mut s = move_to_dst(&case);
        s.execute("slide.layout", &json!({"layout": "SRC"})).expect("back");
        let c = shape_with(&s, "C");
        assert_eq!((c.ph.as_ref().map(|p| p.idx), c.xfrm), (Some(11), None));
    }

    #[test]
    fn a_change_of_theme_leaves_a_placeholder_the_new_layout_lacks_where_it_is_drawn() {
        // PowerPoint copies the slide's layout into the new design, so nothing is left over and nothing
        // moves; here the leftover keeps the frame it was drawn at and its idx.
        let case = Case { width: 960.0, b: frame(60.0, 405.0), c: frame(495.0, 405.0), target: vec![(10, frame(60.0, 840.0))], explicit: None };
        let mut s = seeded(&case);
        s.edit(|doc, _| {
            let dst = doc.masters.iter().flat_map(|m| m.layouts.iter()).find(|l| l.name == "DST").map(|l| l.id).expect("DST");
            super::apply_layout_in_place(doc, 0, dst)
        })
        .expect("apply");
        let c = shape_with(&s, "C");
        assert_eq!((c.ph.as_ref().map(|p| p.idx), c.xfrm.map(|x| x.x)), (Some(11), Some(495.0)));
    }

    #[test]
    fn a_placeholder_centred_exactly_on_the_slide_goes_to_the_left_edge() {
        // A footer 324 pt wide at 318 on a slide 960 wide went to 0 in PowerPoint.
        let case = Case { width: 960.0, b: frame(60.0, 840.0), c: frame(318.0, 324.0), target: vec![(10, frame(60.0, 840.0))], explicit: None };
        let s = move_to_dst(&case);
        assert_eq!(left_of(&s, "C"), Some(0.0));
    }

    #[test]
    fn a_footer_and_a_slide_number_the_new_layout_lacks_become_orphans_against_the_edges() {
        let mut s = Session::with_new();
        s.execute("design.headerFooter", &json!({"slideNumber": true, "footer": true, "footerText": "FOOT"})).expect("footer");
        s.edit(|doc, _| {
            add_layout(doc, "NOFOOT", &[(10, frame(60.0, 840.0))]);
            Ok(())
        })
        .expect("layout");
        let size = |s: &Session, what: &str| {
            let st = s.doc().expect("doc");
            let slide = &st.doc.slides[st.selection.slide];
            let sh = slide.shapes.iter().find(|x| x.ph.as_ref().is_some_and(|p| p.kind.xml() == what)).cloned().expect("the placeholder");
            (sh.clone(), crate::cmd::xfrm_of(&st.doc, &st.selection, &sh))
        };
        let (_, foot_before) = size(&s, "ftr");
        let (_, num_before) = size(&s, "sldNum");
        s.execute("slide.layout", &json!({"layout": "NOFOOT"})).expect("layout");
        for (what, before) in [("ftr", foot_before), ("sldNum", num_before)] {
            let (sh, after) = size(&s, what);
            assert_eq!(sh.ph.as_ref().map(|p| p.idx), Some(ORPHAN_IDX), "{what} pairs with nothing now");
            let width = s.doc().expect("doc").doc.slide_size.width;
            assert!(after.x == 0.0 || after.x == width - after.w, "{what} is against a side edge: {after:?}");
            assert_eq!((after.y, after.w, after.h), (before.y, before.w, before.h));
        }
    }
}
