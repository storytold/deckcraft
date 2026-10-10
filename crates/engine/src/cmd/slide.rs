//! Slides: new, duplicate, delete, move, layout, reset, hide, notes, sections, navigation.

use std::sync::Arc;

use deckcraft_model::text::TextBody;
use deckcraft_model::{LayoutId, LayoutType, Presentation, Section, SlideId, defaults};
use serde_json::{Value, json};

use super::*;
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
        cmd!("slide.move", "Move Slide", [], None, "{from?: index, to: index}", has_slide, move_slide),
        cmd!(noundo "slide.go", "Go to Slide", [], None, "{index} | {id}", has_slide, go),
        cmd!(noundo "slide.next", "Next Slide", [], Some("PageDown"), "{}", has_slide, next),
        cmd!(noundo "slide.previous", "Previous Slide", [], Some("PageUp"), "{}", has_slide, prev),
        cmd!(noundo "slide.first", "First Slide", [], Some("Home"), "{}", has_slide, first),
        cmd!(noundo "slide.last", "Last Slide", [], Some("End"), "{}", has_slide, last),
        cmd!(noundo "slide.selectSlides", "Select Slides", [], None, "{ids?: [slide id], indices?: [index], add?: bool}", has_slide, select_slides),
        cmd!("slide.hide", "Hide Slide", ["Slide Show", "Set Up"], None, "{index?, hidden?: bool}", has_slide, hide),
        cmd!("slide.layout", "Layout", ["Home", "Slides"], None, "{layout: name or kind, index?}", has_slide, set_layout),
        cmd!("slide.reset", "Reset", ["Home", "Slides"], None, "{index?}", has_slide, reset),
        cmd!("slide.notes", "Notes", ["View"], None, "{text, index?}", has_slide, notes),
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
        cmd!("slide.fromOutline", "Slides from Outline…", ["Insert", "Slides From"], None, "{text}", has_doc, from_outline),
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
        let mut slide = defaults::new_slide(doc, layout);
        if let Some(t) = str_param(p, "title")
            && let Some(sh) = slide.shapes.iter_mut().find(|x| x.ph_type().is_some_and(|k| k.is_title()))
        {
            sh.text = Some(TextBody::from_text(t));
        }
        if let Some(b) = str_param(p, "body")
            && let Some(sh) = slide.shapes.iter_mut().find(|x| x.ph_type().is_some_and(|k| !k.is_title()))
        {
            sh.text = Some(TextBody::from_text(b));
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

fn index_of(s: &Session, p: &Value) -> Result<usize> {
    let st = s.doc()?;
    let i = usize_param(p, "index").unwrap_or(st.selection.slide);
    if i >= st.doc.slides.len() {
        return Err(bad("slide", format!("no slide {i}")));
    }
    Ok(i)
}

fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let indices: Vec<usize> = match usize_param(p, "index") {
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
            copy.id = SlideId(doc.alloc_id());
            super::edit::reid(doc, &mut copy.shapes);
            // Animations follow the new shape ids by position.
            let old_ids: Vec<_> = doc
                .slides
                .get(*i)
                .map(|x| {
                    let mut v = vec![];
                    deckcraft_model::walk(&x.shapes, &mut |sh, _| v.push(sh.id));
                    v
                })
                .unwrap_or_default();
            let mut new_shape_ids = vec![];
            deckcraft_model::walk(&copy.shapes, &mut |sh, _| new_shape_ids.push(sh.id));
            for a in &mut copy.animations {
                if let Some(pos) = old_ids.iter().position(|x| *x == a.shape) {
                    a.shape = new_shape_ids.get(pos).copied().unwrap_or(a.shape);
                }
            }
            let section = doc.section_of(src_id(doc, *i));
            let at = (last + 1 + k).min(doc.slides.len());
            new_ids.push(copy.id);
            let nid = copy.id;
            doc.slides.insert(at, Arc::new(copy));
            if let Some(si) = section
                && let Some(sec) = doc.sections.get_mut(si)
            {
                sec.slides.push(nid);
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
        } else if let Some(i) = usize_param(p, "index") {
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
        sel.slide = first.min(doc.slides.len().saturating_sub(1));
        sel.slides = doc.slides.get(sel.slide).map(|x| vec![x.id]).unwrap_or_default();
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"deleted": ids.len(), "remaining": doc.slides.len()}))
    })
}

fn move_slide(s: &mut Session, p: &Value) -> Result<Value> {
    let from = usize_param(p, "from").unwrap_or(s.doc()?.selection.slide);
    let to = usize_param(p, "to").ok_or_else(|| bad("slide.move", "missing `to`"))?;
    s.edit(|doc, sel| {
        if from >= doc.slides.len() {
            return Err(bad("slide.move", format!("no slide {from}")));
        }
        let sl = doc.slides.remove(from);
        let to = to.min(doc.slides.len());
        let id = sl.id;
        doc.slides.insert(to, sl);
        // Keep sections consistent with order: move the id into the section of its new neighbour.
        if !doc.sections.is_empty() {
            for sec in &mut doc.sections {
                sec.slides.retain(|x| *x != id);
            }
            let neighbour = to.checked_sub(1).and_then(|i| doc.slides.get(i)).map(|x| x.id);
            let si = neighbour.and_then(|n| doc.section_of(n)).unwrap_or(0);
            let order: Vec<SlideId> = doc.slides.iter().map(|x| x.id).collect();
            if let Some(sec) = doc.sections.get_mut(si) {
                sec.slides.push(id);
                sec.slides.sort_by_key(|x| order.iter().position(|o| o == x).unwrap_or(usize::MAX));
            }
        }
        sel.slide = to;
        sel.slides = vec![id];
        Ok(json!({"index": to}))
    })
}

fn go_to(s: &mut Session, i: usize) -> Result<Value> {
    s.select(|doc, sel| {
        let n = doc.slides.len();
        sel.slide = i.min(n.saturating_sub(1));
        sel.slides = doc.slides.get(sel.slide).map(|x| vec![x.id]).unwrap_or_default();
        sel.shapes.clear();
        sel.text = None;
        sel.cells = None;
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
            for i in &ids {
                if !sel.slides.contains(i) {
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
    })?;
    Ok(json!({"selected": s.doc()?.selection.slides}))
}

fn hide(s: &mut Session, p: &Value) -> Result<Value> {
    let targets: Vec<usize> = match usize_param(p, "index") {
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

/// Re-map a slide's placeholders onto a new layout (content kept, matched by type/index).
pub fn apply_layout(doc: &mut Presentation, index: usize, layout: LayoutId) -> Result<()> {
    let lay_shapes: Vec<deckcraft_model::Shape> =
        doc.layout(layout).map(|(_, l)| l.shapes.clone()).ok_or_else(|| bad("slide.layout", "no such layout"))?;
    let mut fresh = vec![];
    for ls in lay_shapes.iter().filter(|x| x.ph.as_ref().is_some_and(|p| !p.kind.is_footer_kind())) {
        let id = deckcraft_model::ShapeId(doc.alloc_id());
        fresh.push((id, ls.clone()));
    }
    let slide = doc.slides.get_mut(index).ok_or_else(|| bad("slide.layout", "no slide"))?;
    let slide = Arc::make_mut(slide);
    let mut old: Vec<deckcraft_model::Shape> = std::mem::take(&mut slide.shapes);
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
    // Leftover placeholders with content stay as they were (positioned); empty ones go.
    for o in old {
        let empty_ph = o.ph.is_some() && o.text.as_ref().is_none_or(|t| t.is_empty()) && matches!(o.kind, deckcraft_model::ShapeKind::Shape);
        if !empty_ph {
            out.push(o);
        }
    }
    slide.shapes = out;
    slide.layout = layout;
    Ok(())
}

fn set_layout(s: &mut Session, p: &Value) -> Result<Value> {
    let key = p.get("layout").cloned().ok_or_else(|| bad("slide.layout", "missing `layout`"))?;
    let targets: Vec<usize> = match usize_param(p, "index") {
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
        if let Some(sl) = doc.slides.get_mut(i) {
            let sl = Arc::make_mut(sl);
            sl.background = None;
            for sh in &mut sl.shapes {
                if sh.ph.is_some() {
                    sh.xfrm = None;
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
    let text = str_param(p, "text").unwrap_or("").to_string();
    s.edit(|doc, _| {
        if let Some(sl) = doc.slides.get_mut(i) {
            Arc::make_mut(sl).notes = TextBody::from_text(&text);
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
    let at = usize_param(p, "at").unwrap_or(s.doc()?.selection.slide);
    let name = str_param(p, "name").unwrap_or("Untitled Section").to_string();
    s.edit(|doc, _| {
        let at = at.min(doc.slides.len().saturating_sub(1));
        let ids: Vec<SlideId> = doc.slides.iter().map(|x| x.id).collect();
        if doc.sections.is_empty() {
            // The slides before `at` go into a "Default Section".
            if at > 0 {
                doc.sections.push(Section { name: "Default Section".into(), slides: ids.get(..at).map(|x| x.to_vec()).unwrap_or_default() });
            }
            doc.sections.push(Section { name, slides: ids.get(at..).map(|x| x.to_vec()).unwrap_or_default() });
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
    s.edit(|doc, sel| {
        let before = doc.slides.len();
        let n = deckcraft_format::outline_to_slides(doc, &text);
        // outline_to_slides appends; move them after the current slide.
        let at = (sel.slide + 1).min(before);
        let new: Vec<_> = doc.slides.drain(before..).collect();
        for (k, sl) in new.into_iter().enumerate() {
            doc.slides.insert(at + k, sl);
        }
        Ok(json!({"slides": n}))
    })
}
