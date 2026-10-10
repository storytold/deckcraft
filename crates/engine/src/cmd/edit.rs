//! Edit menu: undo/redo, clipboard, duplicate, delete, select all.

use std::sync::Arc;

use deckcraft_model::{MediaId, Shape, ShapeId, Slide, SlideId};
use serde_json::{Value, json};

use super::*;
use crate::{HistoryEntry, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "edit.undo", "Undo", ["Edit"], Some("Cmd+Z"), "{}", can_undo, undo),
        cmd!(noundo "edit.redo", "Redo", ["Edit"], Some("Cmd+Y"), "{}", can_redo, redo),
        cmd!("edit.cut", "Cut", ["Home", "Clipboard"], Some("Cmd+X"), "{scope?: slides}", has_doc, cut),
        cmd!(noundo "edit.copy", "Copy", ["Home", "Clipboard"], Some("Cmd+C"), "{scope?: slides} → {text}", has_doc, copy),
        cmd!("edit.paste", "Paste", ["Home", "Clipboard"], Some("Cmd+V"), "{text?: plain text to paste, scope?: slides}", has_doc, paste),
        cmd!("edit.pasteText", "Paste and Match Formatting", ["Edit"], Some("Cmd+Shift+V"), "{text?}", has_doc, paste_text),
        cmd!("edit.duplicate", "Duplicate", ["Edit"], Some("Cmd+D"), "{ids?}", has_selection, duplicate),
        cmd!("edit.delete", "Delete", ["Edit"], Some("Delete"), "{ids?, scope?: slides}", has_doc, delete),
        cmd!(noundo "edit.selectAll", "Select All", ["Edit"], Some("Cmd+A"), "{}", has_slide, select_all),
        cmd!(noundo "edit.deselect", "Deselect", [], Some("Escape"), "{}", has_doc, deselect),
        cmd!(noundo "edit.select", "Select Objects", [], None, "{ids: [id], add?: bool, toggle?: bool}", has_slide, select),
    ]
}

fn restore(s: &mut Session, redo: bool) -> Result<Value> {
    let st = s.doc_mut()?;
    let (from, to) = if redo { (&mut st.history.redo, &mut st.history.undo) } else { (&mut st.history.undo, &mut st.history.redo) };
    let Some(e) = from.pop() else { return ok() };
    to.push(HistoryEntry { label: e.label.clone(), doc: st.doc.clone(), selection: st.selection.clone() });
    st.doc = e.doc;
    st.selection = e.selection;
    st.interaction = None;
    st.revision += 1;
    Ok(json!({"label": e.label}))
}

fn undo(s: &mut Session, _p: &Value) -> Result<Value> {
    restore(s, false)
}
fn redo(s: &mut Session, _p: &Value) -> Result<Value> {
    restore(s, true)
}

fn slides_scope(s: &Session, p: &Value) -> bool {
    str_param(p, "scope") == Some("slides")
        || s.active().is_some_and(|d| d.selection.shapes.is_empty() && d.selection.text.is_none() && !d.selection.slides.is_empty())
}

fn media_of_shapes(shapes: &[Shape], out: &mut Vec<MediaId>) {
    deckcraft_model::walk(shapes, &mut |s, _| {
        match &s.kind {
            deckcraft_model::ShapeKind::Picture { fill } => out.push(fill.media),
            deckcraft_model::ShapeKind::Media(m) => {
                out.push(m.media);
                if let Some(p) = m.poster {
                    out.push(p);
                }
            }
            _ => {}
        }
        if let Some(deckcraft_model::Fill::Picture(pf)) = &s.fill {
            out.push(pf.media);
        }
    });
}

fn copy_inner(s: &mut Session, p: &Value) -> Result<String> {
    let st = s.doc()?;
    // Text selection.
    if let Some(t) = &st.selection.text {
        if t.is_range() {
            let body = super::text::body_of(st, t).cloned().unwrap_or_default();
            let (a, b) = t.ordered();
            let plain = deckcraft_model::edit::text_range(&body, a, b);
            let sliced = deckcraft_model::edit::slice(&body, a, b);
            s.clipboard = crate::Clipboard { text: Some(sliced), plain: plain.clone(), ..Default::default() };
            return Ok(plain);
        }
        return Ok(String::new());
    }
    if slides_scope(s, p) {
        let ids: Vec<SlideId> =
            if st.selection.slides.is_empty() { st.current_slide().map(|x| vec![x.id]).unwrap_or_default() } else { st.selection.slides.clone() };
        let slides: Vec<Slide> = ids.iter().filter_map(|id| st.doc.slide(*id).cloned()).collect();
        let mut mids = vec![];
        for sl in &slides {
            media_of_shapes(&sl.shapes, &mut mids);
        }
        let media = st.doc.media.iter().filter(|m| mids.contains(&m.id)).cloned().collect();
        let plain = slides.iter().map(|x| x.title()).collect::<Vec<_>>().join("\n");
        s.clipboard = crate::Clipboard { slides, media, plain: plain.clone(), ..Default::default() };
        return Ok(plain);
    }
    let shapes: Vec<Shape> = st
        .selection
        .shapes
        .iter()
        .filter_map(|id| {
            st.shape(*id).cloned().map(|mut sh| {
                if sh.xfrm.is_none() {
                    sh.xfrm = Some(xfrm_of(&st.doc, &st.selection, &sh));
                }
                sh
            })
        })
        .collect();
    let mut mids = vec![];
    media_of_shapes(&shapes, &mut mids);
    let media = st.doc.media.iter().filter(|m| mids.contains(&m.id)).cloned().collect();
    let plain = shapes.iter().filter_map(|x| x.text.as_ref().map(|t| t.text())).collect::<Vec<_>>().join("\n");
    s.clipboard = crate::Clipboard { shapes, media, plain: plain.clone(), ..Default::default() };
    Ok(plain)
}

fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    let t = copy_inner(s, p)?;
    Ok(json!({"text": t}))
}

fn cut(s: &mut Session, p: &Value) -> Result<Value> {
    let t = copy_inner(s, p)?;
    delete(s, p)?;
    Ok(json!({"text": t}))
}

/// Give pasted shapes fresh ids (groups included) and map connector ends.
pub(crate) fn reid(doc: &mut deckcraft_model::Presentation, shapes: &mut [Shape]) {
    fn rec(doc: &mut deckcraft_model::Presentation, shapes: &mut [Shape], depth: usize) {
        if depth > 64 {
            return;
        }
        for s in shapes {
            s.id = ShapeId(doc.alloc_id());
            if let Some(ch) = s.children_mut() {
                rec(doc, ch, depth + 1);
            }
        }
    }
    rec(doc, shapes, 0);
}

fn bring_media(doc: &mut deckcraft_model::Presentation, media: &[deckcraft_model::MediaItem]) {
    for m in media {
        if doc.media(m.id).is_none_or(|x| x.data != m.data) {
            if doc.media(m.id).is_none() {
                doc.media.push(m.clone());
            } else {
                // Id clash with different bytes: unusual (other deck); keep ours, add theirs as new.
                let id = doc.add_media(&m.name, &m.content_type, m.data.as_ref().clone());
                let _ = id;
            }
        }
    }
}

fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    // External plain text (from the system clipboard) wins when given and differs from ours.
    if let Some(t) = str_param(p, "text")
        && t != s.clipboard.plain
    {
        return paste_plain(s, t);
    }
    let editing = s.doc()?.selection.text.is_some();
    if editing {
        if let Some(body) = s.clipboard.text.clone() {
            return super::text::paste_body(s, &body);
        }
        let plain = s.clipboard.plain.clone();
        return paste_plain(s, &plain);
    }
    let clip = s.clipboard.clone();
    if !clip.slides.is_empty() {
        let at = s.doc()?.selection.slide + 1;
        return s.edit(|doc, sel| {
            bring_media(doc, &clip.media);
            let mut new_ids = vec![];
            for (k, sl) in clip.slides.iter().enumerate() {
                let mut sl = sl.clone();
                sl.id = SlideId(doc.alloc_id());
                reid(doc, &mut sl.shapes);
                if doc.layout(sl.layout).is_none() {
                    sl.layout = doc.masters.first().and_then(|m| m.layouts.first()).map(|l| l.id).unwrap_or_default();
                }
                new_ids.push(sl.id);
                let i = (at + k).min(doc.slides.len());
                doc.slides.insert(i, Arc::new(sl));
            }
            sel.slide = (at + new_ids.len()).saturating_sub(1).min(doc.slides.len().saturating_sub(1));
            sel.slides = new_ids.clone();
            sel.shapes.clear();
            Ok(json!({"slides": new_ids.len()}))
        });
    }
    if !clip.shapes.is_empty() {
        s.clipboard.pastes += 1;
        let off = s.prefs.duplicate_offset * s.clipboard.pastes as f64;
        // Pasting on another slide keeps the position; on the same slide it offsets.
        let same_slide = s.doc().ok().is_some_and(|d| clip.shapes.iter().any(|c| d.shape(c.id).is_some()));
        let off = if same_slide { off } else { 0.0 };
        return s.edit(|doc, sel| {
            bring_media(doc, &clip.media);
            let mut shapes = clip.shapes.clone();
            reid(doc, &mut shapes);
            for sh in &mut shapes {
                if let Some(x) = sh.xfrm.as_mut() {
                    x.x += off;
                    x.y += off;
                }
                // A pasted placeholder becomes a regular shape keeping its look.
                if sh.ph.is_some() && sel.target == crate::Target::Slides {
                    sh.ph = None;
                }
            }
            let ids: Vec<ShapeId> = shapes.iter().map(|x| x.id).collect();
            let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("edit.paste", "no slide"))?;
            list.extend(shapes);
            sel.shapes = ids.clone();
            sel.text = None;
            Ok(json!({"ids": ids}))
        });
    }
    if !clip.plain.is_empty() {
        let t = clip.plain.clone();
        return paste_plain(s, &t);
    }
    ok()
}

fn paste_plain(s: &mut Session, text: &str) -> Result<Value> {
    if s.doc()?.selection.text.is_some() {
        return super::text::insert_text(s, text, None);
    }
    // No text box being edited: a new text box with the text.
    let st = s.doc()?;
    let size = st.doc.slide_size;
    let rect = [size.width * 0.25, size.height * 0.4, size.width * 0.5, 40.0];
    s.execute("insert.textBox", &json!({"rect": rect, "text": text}))
}

fn paste_text(s: &mut Session, p: &Value) -> Result<Value> {
    let t = str_param(p, "text").map(String::from).unwrap_or_else(|| s.clipboard.plain.clone());
    paste_plain(s, &t)
}

fn duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let off = s.prefs.duplicate_offset;
    s.edit(|doc, sel| {
        let list = super::current_shapes(doc, sel).cloned().unwrap_or_default();
        let mut copies: Vec<Shape> = list.iter().filter(|x| ids.contains(&x.id)).cloned().collect();
        let boxes: Vec<_> = copies.iter().map(|c| xfrm_of(doc, sel, c)).collect();
        reid(doc, &mut copies);
        for (c, b) in copies.iter_mut().zip(boxes) {
            let mut x = b;
            x.x += off;
            x.y += off;
            c.xfrm = Some(x);
            if sel.target == crate::Target::Slides {
                c.ph = None;
            }
            if !c.name.is_empty() {
                c.name = format!("{} copy", c.name.trim_end_matches(" copy"));
            }
        }
        let new_ids: Vec<ShapeId> = copies.iter().map(|c| c.id).collect();
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("edit.duplicate", "no slide"))?;
        shapes.extend(copies);
        sel.shapes = new_ids.clone();
        Ok(json!({"ids": new_ids}))
    })
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    if s.doc()?.selection.text.is_some() && ids_param(p, "ids").is_none() {
        return super::text::delete_dir(s, "forward");
    }
    if slides_scope(s, p) && ids_param(p, "ids").is_none() {
        return s.execute("slide.delete", &json!({}));
    }
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return ok();
    }
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("edit.delete", "no slide"))?;
        let n = remove_shapes(shapes, &ids);
        // Animations of deleted shapes go too.
        if sel.target == crate::Target::Slides
            && let Some(sl) = doc.slides.get_mut(sel.slide)
        {
            Arc::make_mut(sl).animations.retain(|a| !ids.contains(&a.shape));
        }
        sel.shapes.retain(|i| !ids.contains(i));
        sel.text = None;
        Ok(json!({"deleted": n}))
    })
}

/// Remove shapes by id anywhere in the tree; empty groups go too.
pub(crate) fn remove_shapes(shapes: &mut Vec<Shape>, ids: &[ShapeId]) -> usize {
    let before = count(shapes);
    fn rec(v: &mut Vec<Shape>, ids: &[ShapeId], depth: usize) {
        v.retain(|s| !ids.contains(&s.id));
        if depth > 64 {
            return;
        }
        for s in v.iter_mut() {
            if let Some(ch) = s.children_mut() {
                rec(ch, ids, depth + 1);
            }
        }
        v.retain(|s| !s.is_group() || !s.children().is_empty());
    }
    rec(shapes, ids, 0);
    before.saturating_sub(count(shapes))
}

fn count(v: &[Shape]) -> usize {
    let mut n = 0;
    deckcraft_model::walk(v, &mut |_, _| n += 1);
    n
}

fn select_all(s: &mut Session, _p: &Value) -> Result<Value> {
    if s.doc()?.selection.text.is_some() {
        return s.execute("text.selectAll", &json!({}));
    }
    s.select(|_, sel| {
        sel.text = None;
    })?;
    let ids: Vec<ShapeId> = s.doc()?.shapes().iter().filter(|x| !x.hidden && !x.locked).map(|x| x.id).collect();
    s.select(|_, sel| sel.shapes = ids.clone())?;
    Ok(json!({"ids": ids}))
}

fn deselect(s: &mut Session, _p: &Value) -> Result<Value> {
    s.painter = None;
    s.anim_painter = None;
    s.select(|_, sel| {
        if let Some(t) = sel.text.take() {
            // Esc while editing text selects the shape.
            if !t.notes {
                sel.shapes = vec![t.shape];
            }
        } else {
            sel.shapes.clear();
            sel.cells = None;
        }
    })?;
    ok()
}

fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = match ids_param(p, "ids") {
        Some(v) => v,
        None => id_param(p, "id", "edit.select")?.map(|i| vec![i]).unwrap_or_default(),
    };
    let add = bool_or(p, "add", false);
    let toggle = bool_or(p, "toggle", false);
    let valid: Vec<ShapeId> = ids.into_iter().filter(|i| s.active().is_some_and(|d| d.shape(*i).is_some())).collect();
    s.select(|_, sel| {
        sel.text = None;
        sel.cells = None;
        if toggle {
            for i in &valid {
                if let Some(k) = sel.shapes.iter().position(|x| x == i) {
                    sel.shapes.remove(k);
                } else {
                    sel.shapes.push(*i);
                }
            }
        } else if add {
            for i in &valid {
                if !sel.shapes.contains(i) {
                    sel.shapes.push(*i);
                }
            }
        } else {
            sel.shapes = valid.clone();
        }
    })?;
    Ok(json!({"selected": s.doc()?.selection.shapes}))
}
