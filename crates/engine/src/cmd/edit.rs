//! Edit menu: undo/redo, clipboard, duplicate, delete, select all.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_model::text::TextBody;
use deckcraft_model::{MediaId, Shape, ShapeId, Slide, SlideId};
use serde_json::{Value, json};

use super::*;
use crate::{HistoryEntry, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "edit.undo", "Undo", ["Edit"], Some("Cmd+Z"), "{}", can_undo, undo),
        cmd!(noundo "edit.redo", "Redo", ["Edit"], Some("Cmd+Y"), "{}", can_redo, redo),
        cmd!("edit.cut", "Cut", ["Home", "Clipboard"], Some("Cmd+X"), "{scope?: slides} → {text, html?}", has_doc, cut),
        cmd!(noundo "edit.copy", "Copy", ["Home", "Clipboard"], Some("Cmd+C"), "{scope?: slides} → {text, html?}", has_doc, copy),
        cmd!(
            "edit.paste",
            "Paste",
            ["Home", "Clipboard"],
            Some("Cmd+V"),
            "{text?: plain text to paste, html?: rich text from another app, scope?: slides}",
            has_doc,
            paste
        ),
        cmd!(
            "edit.pasteSpecial",
            "Paste Special",
            ["Home", "Clipboard"],
            None,
            "{as: destinationTheme|keepSource|picture|text} — Use Destination Theme, Keep Source Formatting, Picture (PNG), Keep Text Only",
            has_doc,
            paste_special
        ),
        cmd!("edit.pasteText", "Paste and Match Formatting", ["Edit"], Some("Cmd+Shift+V"), "{text?}", has_doc, paste_text),
        cmd!("edit.duplicate", "Duplicate", ["Edit"], Some("Cmd+D"), "{ids?}", has_selection, duplicate),
        cmd!("edit.delete", "Delete", ["Edit"], Some("Delete"), "{ids?, scope?: slides}", has_doc, delete),
        cmd!(noundo "edit.selectAll", "Select All", ["Edit"], Some("Cmd+A"), "{scope?: slides}", has_slide, select_all),
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

pub(crate) fn media_of_shapes(shapes: &[Shape], out: &mut Vec<MediaId>) {
    deckcraft_model::walk(shapes, &mut |s, _| {
        match &s.kind {
            deckcraft_model::ShapeKind::Picture { fill } => out.push(fill.media),
            deckcraft_model::ShapeKind::Media(m) => {
                out.push(m.media);
                if let Some(p) = m.poster {
                    out.push(p);
                }
            }
            deckcraft_model::ShapeKind::Model3d { media, poster } => {
                out.push(*media);
                if let Some(p) = poster {
                    out.push(*p);
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
            if let Some(deckcraft_model::Background::Fill { fill: deckcraft_model::Fill::Picture(pf) }) = &sl.background {
                mids.push(pf.media);
            }
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
    let source_theme = st.current_slide().and_then(|sl| st.doc.master_for(sl)).map(|(m, _)| (m.scheme(), m.theme.fonts.clone()));
    s.clipboard = crate::Clipboard { shapes, media, plain: plain.clone(), source_theme, ..Default::default() };
    Ok(plain)
}

fn copying_caret(s: &Session) -> bool {
    s.active().is_some_and(|d| d.selection.text.as_ref().is_some_and(|t| !t.is_range()))
}

fn copy(s: &mut Session, p: &Value) -> Result<Value> {
    // A caret copies nothing; the previous clipboard must not leak into this answer.
    let caret = copying_caret(s);
    let t = copy_inner(s, p)?;
    let html = if caret { String::new() } else { copied_html(s) };
    Ok(json!({"text": t, "html": html}))
}

/// HTML for what was just copied, when it is text (a selection, or the text on the copied shapes).
fn copied_html(s: &Session) -> String {
    if let Some(body) = &s.clipboard.text {
        return richclip::body_to_html(body);
    }
    let mut paragraphs = Vec::new();
    for sh in &s.clipboard.shapes {
        if let Some(t) = &sh.text {
            paragraphs.extend(t.paragraphs.clone());
        }
    }
    if paragraphs.is_empty() {
        return String::new();
    }
    richclip::body_to_html(&TextBody { paragraphs, ..Default::default() })
}

fn cut(s: &mut Session, p: &Value) -> Result<Value> {
    let caret = copying_caret(s);
    let t = copy_inner(s, p)?;
    if caret {
        // Nothing is selected, so there is nothing to cut; `delete` would remove the next character.
        return Ok(json!({"text": t, "html": ""}));
    }
    let html = copied_html(s);
    delete(s, p)?;
    Ok(json!({"text": t, "html": html}))
}

/// Give pasted shapes fresh ids (groups included) and return the old-to-new map. Connector ends
/// attached to a shape that was copied too follow its copy; ends attached to a shape outside the
/// copied set are detached (the connector keeps its geometry), so a copy never stays glued to
/// the original or to an unrelated shape that happens to share an id in another deck.
pub(crate) fn reid(cmd: &str, doc: &mut deckcraft_model::Presentation, shapes: &mut [Shape]) -> Result<HashMap<ShapeId, ShapeId>> {
    fn rec(
        cmd: &str,
        doc: &mut deckcraft_model::Presentation,
        shapes: &mut [Shape],
        map: &mut HashMap<ShapeId, ShapeId>,
        depth: usize,
    ) -> Result<()> {
        if depth > 64 {
            return Ok(());
        }
        for s in shapes {
            let id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
            map.insert(s.id, id);
            s.id = id;
            if let Some(ch) = s.children_mut() {
                rec(cmd, doc, ch, map, depth + 1)?;
            }
        }
        Ok(())
    }
    let mut map = HashMap::new();
    rec(cmd, doc, shapes, &mut map, 0)?;
    walk_mut(shapes, &mut |s| {
        if let deckcraft_model::ShapeKind::Connector { start, end } = &mut s.kind {
            for e in [start, end] {
                *e = e.and_then(|(id, site)| map.get(&id).map(|n| (*n, site)));
            }
        }
    });
    Ok(map)
}

/// Point copied animations at the copies: targets and triggers in `map` follow it; an animation
/// whose target wasn't copied is dropped, and a trigger that wasn't copied is cleared.
pub(crate) fn remap_animations(anims: &mut Vec<deckcraft_model::Animation>, map: &HashMap<ShapeId, ShapeId>) {
    anims.retain_mut(|a| {
        let Some(n) = map.get(&a.shape) else { return false };
        a.shape = *n;
        a.trigger = a.trigger.and_then(|t| map.get(&t).copied());
        true
    });
}

/// Visit every shape mutably, depth-first (groups included, depth-bounded like `walk`).
fn walk_mut(shapes: &mut [Shape], f: &mut dyn FnMut(&mut Shape)) {
    fn rec(shapes: &mut [Shape], f: &mut dyn FnMut(&mut Shape), depth: usize) {
        if depth > 64 {
            return;
        }
        for s in shapes {
            f(s);
            if let Some(ch) = s.children_mut() {
                rec(ch, f, depth + 1);
            }
        }
    }
    rec(shapes, f, 0);
}

/// Rewrite the media ids shapes use (`map`: clipboard id to destination id).
fn remap_media(shapes: &mut [Shape], map: &HashMap<MediaId, MediaId>) {
    if map.is_empty() {
        return;
    }
    let m = |id: &mut MediaId| {
        if let Some(n) = map.get(id) {
            *id = *n;
        }
    };
    walk_mut(shapes, &mut |s| {
        match &mut s.kind {
            deckcraft_model::ShapeKind::Picture { fill } => m(&mut fill.media),
            deckcraft_model::ShapeKind::Media(c) => {
                m(&mut c.media);
                if let Some(p) = c.poster.as_mut() {
                    m(p);
                }
            }
            deckcraft_model::ShapeKind::Opaque { preview: Some(p), .. } => m(p),
            _ => {}
        }
        if let Some(deckcraft_model::Fill::Picture(pf)) = &mut s.fill {
            m(&mut pf.media);
        }
    });
}

/// Add the clipboard's media to `doc` and return the ids that changed: an id already used in
/// `doc` by other bytes (another deck) gets a fresh id, and the pasted shapes must use it.
fn bring_media(cmd: &str, doc: &mut deckcraft_model::Presentation, media: &[deckcraft_model::MediaItem]) -> Result<HashMap<MediaId, MediaId>> {
    let mut map = HashMap::new();
    for m in media {
        match doc.media(m.id) {
            None => doc.media.push(m.clone()),
            Some(x) if x.data == m.data => {}
            Some(_) => {
                let id = doc.add_media(&m.name, &m.content_type, m.data.as_ref().clone()).ok_or_else(|| bad(cmd, NO_ID_SPACE))?;
                map.insert(m.id, id);
            }
        }
    }
    Ok(map)
}

fn paste(s: &mut Session, p: &Value) -> Result<Value> {
    // External plain text (from the system clipboard) wins when given and differs from ours.
    // HTML from Pages or Keynote keeps bold, italic and underline when it says the same words.
    if let Some(t) = str_param(p, "text")
        && t != s.clipboard.plain
    {
        if let Some(html) = str_param(p, "html")
            && let Some(body) = richclip::html_to_body(html)
            && richclip::same_words(&body.text(), t)
        {
            return paste_external_body(s, &body);
        }
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
            let media = bring_media("edit.paste", doc, &clip.media)?;
            let mut new_ids = vec![];
            for (k, sl) in clip.slides.iter().enumerate() {
                let mut sl = sl.clone();
                sl.id = SlideId(doc.alloc_id().ok_or_else(|| bad("edit.paste", NO_ID_SPACE))?);
                let map = reid("edit.paste", doc, &mut sl.shapes)?;
                remap_animations(&mut sl.animations, &map);
                remap_media(&mut sl.shapes, &media);
                if let Some(deckcraft_model::Background::Fill { fill: deckcraft_model::Fill::Picture(pf) }) = &mut sl.background
                    && let Some(n) = media.get(&pf.media)
                {
                    pf.media = *n;
                }
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
            let media = bring_media("edit.paste", doc, &clip.media)?;
            let mut shapes = clip.shapes.clone();
            reid("edit.paste", doc, &mut shapes)?;
            remap_media(&mut shapes, &media);
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

fn paste_external_body(s: &mut Session, body: &TextBody) -> Result<Value> {
    if s.doc()?.selection.text.is_some() {
        return super::text::paste_body(s, body);
    }
    let plain = body.text();
    let st = s.doc()?;
    let size = st.doc.slide_size;
    let rect = [size.width * 0.25, size.height * 0.4, size.width * 0.5, 40.0];
    let made = s.execute("insert.textBox", &json!({"rect": rect, "text": plain}))?;
    let raw = made.get("id").and_then(Value::as_u64).ok_or_else(|| bad("edit.paste", "no text box"))?;
    let id = u32::try_from(raw).map_err(|_| bad("edit.paste", "no text box"))?;
    let body = body.clone();
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("edit.paste", "no slide"))?;
        let sh = deckcraft_model::find_shape_mut(list, ShapeId(id)).ok_or_else(|| bad("edit.paste", "no text box"))?;
        if let Some(t) = sh.text.as_mut() {
            let (wrap, autofit) = (t.body.wrap, t.body.autofit);
            *t = body.clone();
            t.body.wrap = wrap;
            t.body.autofit = autofit;
        }
        sel.shapes = vec![ShapeId(id)];
        sel.text = None;
        Ok(json!({"id": id}))
    })
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

fn paste_special(s: &mut Session, p: &Value) -> Result<Value> {
    match str_param(p, "as").unwrap_or("destinationTheme") {
        "destinationTheme" | "useDestinationTheme" => paste(s, &json!({})),
        "text" | "keepTextOnly" => paste_text(s, &json!({})),
        "keepSource" | "keepSourceFormatting" => {
            let saved = s.clipboard.clone();
            if let Some((scheme, fonts)) = &saved.source_theme {
                bake_theme(&mut s.clipboard.shapes, scheme, fonts);
            }
            let r = paste(s, &json!({}));
            let pastes = s.clipboard.pastes;
            s.clipboard = saved;
            s.clipboard.pastes = pastes;
            r
        }
        "picture" => paste_picture(s),
        other => Err(bad("edit.pasteSpecial", format!("unknown `as`: {other} (destinationTheme, keepSource, picture, text)"))),
    }
}

/// Replace theme colours and theme fonts with the values they have in `scheme` / `fonts`, so the
/// shapes keep their look under another theme.
pub(crate) fn bake_theme(shapes: &mut [Shape], scheme: &deckcraft_color::ColorScheme, fonts: &deckcraft_model::theme::FontScheme) {
    fn rec(v: &mut Value, scheme: &deckcraft_color::ColorScheme, fonts: &deckcraft_model::theme::FontScheme, depth: usize) {
        if depth > 256 {
            return;
        }
        match v {
            Value::Object(o) => {
                if o.get("kind").and_then(Value::as_str) == Some("scheme")
                    && let Some(slot) = o.get("slot").and_then(|x| serde_json::from_value::<deckcraft_color::SchemeSlot>(x.clone()).ok())
                    && slot != deckcraft_color::SchemeSlot::PhClr
                    && let Ok(rgb) = serde_json::to_value(scheme.get(slot))
                {
                    o.clear();
                    o.insert("kind".into(), json!("rgb"));
                    o.insert("rgb".into(), rgb);
                    return;
                }
                for x in o.values_mut() {
                    rec(x, scheme, fonts, depth + 1);
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| rec(x, scheme, fonts, depth + 1)),
            Value::String(t) if t.starts_with('+') => {
                let set = match t.get(1..3) {
                    Some("mj") => &fonts.major,
                    Some("mn") => &fonts.minor,
                    _ => return,
                };
                let face = match t.get(3..) {
                    Some("-lt") => &set.latin,
                    Some("-ea") => &set.ea,
                    Some("-cs") => &set.cs,
                    _ => return,
                };
                if !face.is_empty() {
                    *t = face.clone();
                }
            }
            _ => {}
        }
    }
    for sh in shapes.iter_mut() {
        let Ok(mut v) = serde_json::to_value(&*sh) else { continue };
        rec(&mut v, scheme, fonts, 0);
        if let Ok(baked) = serde_json::from_value::<Shape>(v) {
            *sh = baked;
        }
    }
}

/// Paste Special › Picture: the copied shapes drawn into one PNG picture.
fn paste_picture(s: &mut Session) -> Result<Value> {
    let mut clip = s.clipboard.clone();
    if clip.shapes.is_empty() {
        return Err(bad("edit.pasteSpecial", "copy shapes first to paste them as a picture"));
    }
    if let Some((scheme, fonts)) = &clip.source_theme {
        bake_theme(&mut clip.shapes, scheme, fonts);
    }
    let st = s.doc()?;
    let mut doc = (*st.doc).clone();
    bring_media("edit.pasteSpecial", &mut doc, &clip.media)?;
    let mut slide = st.current_slide().cloned().ok_or_else(|| bad("edit.pasteSpecial", "no slide"))?;
    let xs: Vec<deckcraft_geom::Xfrm> = clip.shapes.iter().filter_map(|x| x.xfrm).collect();
    let b = super::arrange::union_bounds(&xs).ok_or_else(|| bad("edit.pasteSpecial", "nothing to draw"))?;
    let gx = deckcraft_geom::Xfrm::new(b.x0, b.y0, b.width(), b.height());
    let gid = ShapeId(u32::MAX - 1);
    let one = if clip.shapes.len() == 1 {
        let mut sh = clip.shapes[0].clone();
        sh.id = gid;
        sh
    } else {
        Shape { id: gid, xfrm: Some(gx), kind: deckcraft_model::ShapeKind::Group { children: clip.shapes.clone(), child: gx }, ..Default::default() }
    };
    slide.shapes = vec![one];
    const SCALE: f64 = 2.0;
    let img = deckcraft_render::render_shape(&doc, &slide, gid, SCALE).ok_or_else(|| bad("edit.pasteSpecial", "the shapes could not be drawn"))?;
    // render_shape draws the bounds plus an 8 pt margin (room for shadows and outlines).
    let (w, h) = (img.width as f64 / SCALE, img.height as f64 / SCALE);
    let same_slide = clip.shapes.iter().any(|c| st.shape(c.id).is_some());
    let off = if same_slide { s.prefs.duplicate_offset } else { 0.0 };
    let png = img.to_png();
    s.execute("insert.picture", &json!({"data": base64_encode(&png), "name": "Picture.png", "rect": [b.x0 - 8.0 + off, b.y0 - 8.0 + off, w, h]}))
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
        reid("edit.duplicate", doc, &mut copies)?;
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
        let mut before = Vec::new();
        deckcraft_model::walk(shapes, &mut |s, _| before.push(s.id));
        let n = remove_shapes(shapes, &ids);
        // Everything that is gone: the given ids plus group descendants and emptied groups.
        let mut left = Vec::new();
        deckcraft_model::walk(shapes, &mut |s, _| left.push(s.id));
        let mut gone = ids.clone();
        gone.extend(before.into_iter().filter(|i| !left.contains(i)));
        // Animations of deleted shapes go too.
        if sel.target == crate::Target::Slides
            && let Some(sl) = doc.slides.get_mut(sel.slide)
        {
            let sl = Arc::make_mut(sl);
            sl.animations.retain(|a| !gone.contains(&a.shape));
            // Effects triggered by a deleted shape fall back to the main sequence.
            for a in sl.animations.iter_mut().filter(|a| a.trigger.is_some_and(|t| gone.contains(&t))) {
                a.trigger = None;
            }
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

fn select_all(s: &mut Session, p: &Value) -> Result<Value> {
    match p.get("scope") {
        None | Some(Value::Null) => {}
        Some(Value::String(v)) if v == "slides" => return select_all_slides(s),
        Some(_) => return Err(bad("edit.selectAll", "scope must be \"slides\"")),
    }
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

/// Thumbnail-pane / sorter select all: every slide, hidden ones included. The current
/// slide stays where it is (the canvas must not jump), so this is not `slide.selectSlides`.
fn select_all_slides(s: &mut Session) -> Result<Value> {
    if s.doc()?.selection.target != crate::Target::Slides {
        return Err(bad("edit.selectAll", "the slide list is not shown while editing a master"));
    }
    let ids: Vec<SlideId> = s.doc()?.doc.slides.iter().map(|x| x.id).collect();
    s.select(|_, sel| {
        sel.slides = ids.clone();
        sel.shapes.clear();
        sel.text = None;
        sel.cells = None;
    })?;
    Ok(json!({"selected": ids}))
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
