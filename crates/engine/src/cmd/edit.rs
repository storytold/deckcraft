//! Edit menu: undo/redo, clipboard, duplicate, delete, select all.

use std::collections::{BTreeMap, BTreeSet, HashMap};
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

fn notes_master_target(target: crate::Target) -> bool {
    matches!(target, crate::Target::NotesMaster | crate::Target::ImportedNotesMaster { .. })
}

const MAX_NOTES_COPY_SHAPES: usize = 100_000;

/// Count existing and inserted trees with shared fuel, before cloning either one.
/// A copied notes Body cannot establish a second canonical notes owner safely.
fn count_notes_copy_shapes(cmd: &str, shapes: &[Shape], depth: usize, count: &mut usize, limit: usize, inserted: bool) -> Result<()> {
    if depth > 64 {
        return Err(bad(cmd, "notes frame copy nesting exceeds limit"));
    }
    *count = count.checked_add(shapes.len()).filter(|count| *count <= limit).ok_or_else(|| bad(cmd, "notes frame copy shape count exceeds limit"))?;
    for shape in shapes {
        if inserted && shape.ph_type() == Some(deckcraft_model::PhType::Body) {
            return Err(bad(cmd, "copying primary notes body frames is not supported; edit the existing notes body instead"));
        }
        if !shape.children().is_empty() {
            count_notes_copy_shapes(cmd, shape.children(), depth + 1, count, limit, inserted)?;
        }
    }
    Ok(())
}

fn preflight_notes_insertion<'a>(cmd: &str, existing: &[Shape], inserted: impl Iterator<Item = &'a Shape>, limit: usize) -> Result<()> {
    let mut count = 0;
    count_notes_copy_shapes(cmd, existing, 0, &mut count, limit, false)?;
    for shape in inserted {
        count_notes_copy_shapes(cmd, std::slice::from_ref(shape), 0, &mut count, limit, true)?;
    }
    Ok(())
}

fn notes_shape_index<'a>(cmd: &str, shapes: &'a [Shape]) -> Result<BTreeMap<ShapeId, &'a Shape>> {
    fn index<'a>(cmd: &str, shapes: &'a [Shape], depth: usize, count: &mut usize, out: &mut BTreeMap<ShapeId, &'a Shape>) -> Result<()> {
        if depth > 64 {
            return Err(bad(cmd, "notes frame copy nesting exceeds limit"));
        }
        *count = count
            .checked_add(shapes.len())
            .filter(|count| *count <= MAX_NOTES_COPY_SHAPES)
            .ok_or_else(|| bad(cmd, "notes frame copy shape count exceeds limit"))?;
        for shape in shapes {
            if out.insert(shape.id, shape).is_some() {
                return Err(bad(cmd, "notes frame copy has ambiguous shape identities"));
            }
            if !shape.children().is_empty() {
                index(cmd, shape.children(), depth + 1, count, out)?;
            }
        }
        Ok(())
    }
    let mut by_id = BTreeMap::new();
    let mut count = 0;
    index(cmd, shapes, 0, &mut count, &mut by_id)?;
    Ok(by_id)
}

/// Index a notes owner once; selection lookup retains its order and nested selections.
fn selected_notes_shapes<'a>(cmd: &str, shapes: &'a [Shape], ids: &[ShapeId]) -> Result<Vec<&'a Shape>> {
    if ids.len() > MAX_NOTES_COPY_SHAPES {
        return Err(bad(cmd, "notes frame copy selection exceeds limit"));
    }
    let by_id = notes_shape_index(cmd, shapes)?;
    let selected: Vec<_> = ids.iter().filter_map(|id| by_id.get(id).copied()).collect();
    preflight_notes_insertion(cmd, &[], selected.iter().copied(), MAX_NOTES_COPY_SHAPES)?;
    Ok(selected)
}

fn preflight_notes_shape_paste(s: &Session, cmd: &str) -> Result<()> {
    let st = s.doc()?;
    if notes_master_target(st.selection.target) && st.selection.text.is_none() && s.clipboard.slides.is_empty() && !s.clipboard.shapes.is_empty() {
        preflight_notes_insertion(cmd, st.shapes(), s.clipboard.shapes.iter(), MAX_NOTES_COPY_SHAPES)?;
    }
    Ok(())
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
        if st.selection.slides.len() > crate::transfer_policy::MAX_POLICY_PAGES {
            return Err(bad("edit.copy", "too many pages for clipboard transfer"));
        }
        let ids: Vec<SlideId> =
            if st.selection.slides.is_empty() { st.current_slide().map(|x| vec![x.id]).unwrap_or_default() } else { st.selection.slides.clone() };
        if ids.len() > crate::transfer_policy::MAX_POLICY_PAGES
            || st.doc.slides.len() > deckcraft_model::presentation_template::MAX_TEMPLATE_SLIDE_SCAN
        {
            return Err(bad("edit.copy", "too many pages for clipboard transfer"));
        }
        // Index only requested owners in one document pass, avoiding a whole-deck
        // linear lookup for every selected page. Preserve the requested order.
        let requested: BTreeSet<_> = ids.iter().copied().collect();
        let mut owners = BTreeMap::new();
        for slide in &st.doc.slides {
            if requested.contains(&slide.id) {
                owners.entry(slide.id).or_insert(slide.as_ref());
            }
        }
        let selected: Vec<_> = ids.iter().filter_map(|id| owners.get(id).copied()).collect();
        deckcraft_model::comments::validate(&st.doc, true).map_err(|e| bad("edit.copy", e.to_string()))?;
        let mut prepared: Vec<Slide> = selected.iter().map(|s| (*s).clone()).collect();
        crate::comment_transfer::capture(&st.doc, &mut prepared)?;
        let selected: Vec<&Slide> = prepared.iter().collect();
        let notes_capture = crate::notes_transfer::capture(&st.doc, &selected)?;
        let notes = notes_capture.closure;
        let origin = if let Some(notes) = &notes {
            crate::transfer_policy::capture_closed_notes(&st.doc, st.uid, notes)?
        } else {
            crate::transfer_policy::capture_same_document_only(&st.doc, st.uid, selected.iter().copied(), notes_capture.same_document_only)?
        };
        let templates = crate::template_transfer::capture(&st.doc, selected.iter().copied())?;
        let slides: Vec<Slide> = selected.into_iter().cloned().collect();
        let media = if notes.is_some() {
            Vec::new()
        } else {
            let mut mids = vec![];
            for sl in &slides {
                mids.extend(crate::media_refs::slide_media(sl));
            }
            st.doc.media.iter().filter(|m| mids.contains(&m.id)).cloned().collect()
        };
        let plain = slides.iter().map(|x| x.title()).collect::<Vec<_>>().join("\n");
        s.clipboard = crate::Clipboard { slides, templates, notes, origin, media, plain: plain.clone(), ..Default::default() };
        return Ok(plain);
    }
    let origin = crate::transfer_policy::capture(&st.doc, st.uid, std::iter::empty())?;
    let clone_shape = |shape: &Shape| {
        let mut shape = shape.clone();
        if shape.xfrm.is_none() {
            shape.xfrm = Some(xfrm_of(&st.doc, &st.selection, &shape));
        }
        shape
    };
    let shapes: Vec<Shape> = if notes_master_target(st.selection.target) {
        selected_notes_shapes("edit.copy", st.shapes(), &st.selection.shapes)?.into_iter().map(clone_shape).collect()
    } else {
        st.selection.shapes.iter().filter_map(|id| st.shape(*id)).map(clone_shape).collect()
    };
    let mut mids = vec![];
    media_of_shapes(&shapes, &mut mids);
    let media = st.doc.media.iter().filter(|m| mids.contains(&m.id)).cloned().collect();
    let plain = shapes.iter().filter_map(|x| x.text.as_ref().map(|t| t.text())).collect::<Vec<_>>().join("\n");
    let source_theme = st.current_slide().and_then(|sl| st.doc.master_for(sl)).map(|(m, _)| (m.scheme(), m.theme.fonts.clone()));
    s.clipboard = crate::Clipboard { shapes, origin, media, plain: plain.clone(), source_theme, ..Default::default() };
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
    let prior = s.clipboard.clone();
    let t = copy_inner(s, p)?;
    let html = if caret { String::new() } else { copied_html(s) };
    if let Err(error) = delete(s, p) {
        s.clipboard = prior;
        return Err(error);
    }
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
            None => {
                doc.next_id = doc.next_id.max(m.id.0);
                doc.media.push(m.clone());
            }
            Some(x) if x.data == m.data && x.content_type == m.content_type && x.link == m.link => {}
            Some(_) => {
                let id = doc.add_media(&m.name, &m.content_type, m.data.as_ref().clone()).ok_or_else(|| bad(cmd, NO_ID_SPACE))?;
                map.insert(m.id, id);
            }
        }
    }
    Ok(map)
}

fn preflight_clipboard_transfer(s: &Session) -> Result<()> {
    let state = s.doc()?;
    if state.selection.text.is_none() && (!s.clipboard.slides.is_empty() || !s.clipboard.shapes.is_empty()) {
        if let Some(notes) = &s.clipboard.notes
            && !s.clipboard.slides.is_empty()
            && crate::transfer_policy::notes_clipboard_crosses_document(&s.clipboard.origin, state.uid)?
        {
            crate::notes_transfer::preflight(&state.doc, notes, &s.clipboard.slides)?;
        } else {
            crate::transfer_policy::check_clipboard(&s.clipboard.origin, &state.doc, state.uid, s.clipboard.slides.iter())?;
        }
    }
    Ok(())
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
    preflight_clipboard_transfer(s)?;
    preflight_notes_shape_paste(s, "edit.paste")?;
    let notes_cross = s.clipboard.notes.is_some()
        && !s.clipboard.slides.is_empty()
        && crate::transfer_policy::notes_clipboard_crosses_document(&s.clipboard.origin, s.doc()?.uid)?;
    let comments_same_document = crate::transfer_policy::same_document(&s.clipboard.origin, s.doc()?.uid);
    let keep_regular = bool_or(p, "keepSourceNotesTransfer", false);
    let clip = if notes_cross {
        crate::Clipboard {
            slides: s.clipboard.slides.clone(),
            notes: s.clipboard.notes.clone(),
            templates: s.clipboard.templates.clone(),
            ..Default::default()
        }
    } else {
        s.clipboard.clone()
    };
    if !clip.slides.is_empty() {
        let at = s.doc()?.selection.slide + 1;
        return s.edit_resources(|doc, sel| {
            deckcraft_model::comments::normalize(doc).map_err(|e| bad("edit.paste", e.to_string()))?;
            let mut slides = clip.slides.clone();
            if notes_cross {
                crate::notes_transfer::remap(
                    doc,
                    clip.notes.as_ref().ok_or_else(|| bad("edit.paste", "missing captured notes closure"))?,
                    &mut slides,
                    keep_regular,
                )?;
            }
            crate::template_transfer::remap(doc, &clip.templates, &mut slides)?;
            let media = if notes_cross { HashMap::new() } else { bring_media("edit.paste", doc, &clip.media)? };
            let mut new_ids = vec![];
            new_ids.try_reserve_exact(slides.len()).map_err(|_| bad("edit.paste", "cannot allocate inserted slide index"))?;
            doc.slides.try_reserve(slides.len()).map_err(|_| bad("edit.paste", "cannot allocate inserted slides"))?;
            let mut comment_context = crate::comment_transfer::Context::default();
            let mut comment_slides = if comments_same_document { crate::comment_transfer::identity_slides(doc) } else { Default::default() };
            if !notes_cross {
                for slide in &mut slides {
                    let old = slide.id;
                    slide.id = SlideId(doc.alloc_id().ok_or_else(|| bad("edit.paste", NO_ID_SPACE))?);
                    comment_slides.insert(old, slide.id);
                }
            }
            for (k, mut sl) in slides.into_iter().enumerate() {
                if !notes_cross {
                    let map = reid("edit.paste", doc, &mut sl.shapes)?;
                    let comment_shapes = map.iter().map(|(a, b)| (*a, *b)).collect();
                    let comment_media = crate::comment_transfer::identity_media(&sl.comments, &media);
                    crate::comment_transfer::remap(doc, &mut sl.comments, &comment_shapes, &comment_media, &comment_slides, &mut comment_context)?;
                    remap_animations(&mut sl.animations, &map);
                    remap_media(&mut sl.shapes, &media);
                }
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
        let same_slide = if notes_master_target(s.doc()?.selection.target) {
            let source = notes_shape_index("edit.paste", s.doc()?.shapes())?;
            clip.shapes.iter().any(|shape| source.contains_key(&shape.id))
        } else {
            s.doc().ok().is_some_and(|d| clip.shapes.iter().any(|c| d.shape(c.id).is_some()))
        };
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
            preflight_clipboard_transfer(s)?;
            preflight_notes_shape_paste(s, "edit.pasteSpecial")?;
            if s.clipboard.notes.is_some()
                && !s.clipboard.slides.is_empty()
                && crate::transfer_policy::notes_clipboard_crosses_document(&s.clipboard.origin, s.doc()?.uid)?
            {
                return paste(s, &json!({"keepSourceNotesTransfer":true}));
            }
            let saved = s.clipboard.clone();
            if let Some((scheme, fonts)) = &saved.source_theme {
                bake_theme(&mut s.clipboard.shapes, scheme, fonts);
            }
            let r = paste(s, &json!({"keepSourceNotesTransfer":true}));
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
    let st = s.doc()?;
    let checks_notes = notes_master_target(st.selection.target)
        || (st.selection.target == crate::Target::Slides && st.current_slide().is_some_and(|slide| slide.notes_page.is_some()));
    if checks_notes && ids.len() > MAX_NOTES_COPY_SHAPES {
        return Err(bad("edit.duplicate", "notes frame copy selection exceeds limit"));
    }
    let wanted: BTreeSet<_> = ids.iter().copied().collect();
    if notes_master_target(st.selection.target) {
        preflight_notes_insertion(
            "edit.duplicate",
            st.shapes(),
            st.shapes().iter().filter(|shape| wanted.contains(&shape.id)),
            MAX_NOTES_COPY_SHAPES,
        )?;
    } else if st.selection.target == crate::Target::Slides
        && let Some(page) = st.current_slide().and_then(|slide| slide.notes_page.as_ref())
    {
        // Shape identities are owner-scoped. Actual slide-owned shapes take precedence
        // over numerically coincident notes-frame ids, including nested slide shapes.
        let slide_shapes = notes_shape_index("edit.duplicate", st.shapes())?;
        let unresolved: Vec<_> = ids.iter().filter(|id| !slide_shapes.contains_key(id)).copied().collect();
        // Only requests outside the live slide tree can refer to raw out-of-line frames.
        if page.body_shape.is_some_and(|body| unresolved.contains(&body)) {
            return Err(bad("edit.duplicate", "copying primary notes body frames is not supported; edit the existing notes body instead"));
        }
        selected_notes_shapes("edit.duplicate", &page.shapes, &unresolved)?;
    }
    let off = s.prefs.duplicate_offset;
    s.edit(|doc, sel| {
        let mut copies: Vec<Shape> =
            super::current_shapes(doc, sel).into_iter().flatten().filter(|shape| wanted.contains(&shape.id)).cloned().collect();
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

#[cfg(test)]
mod notes_copy_budget_tests {
    use super::*;

    fn group(children: Vec<Shape>) -> Shape {
        Shape { kind: deckcraft_model::ShapeKind::Group { children, child: Xfrm::new(0., 0., 10., 10.) }, ..Default::default() }
    }

    #[test]
    fn destination_plus_multiple_selected_trees_share_one_count_budget() {
        let existing = vec![group(vec![Shape::default()])]; // Two recursive destination shapes.
        let inserted = [Shape::default(), Shape::default()];
        assert!(preflight_notes_insertion("edit.duplicate", &existing, inserted.iter(), 4).is_ok());
        assert!(preflight_notes_insertion("edit.duplicate", &existing, inserted.iter(), 3).is_err());
        assert_eq!(MAX_NOTES_COPY_SHAPES, 100_000);
    }

    #[test]
    fn pasted_nested_descendants_count_against_existing_destination_before_clone() {
        let existing = vec![Shape::default(), Shape::default()];
        let inserted = [group(vec![group(vec![Shape::default()])])]; // Three inserted shapes.
        assert!(preflight_notes_insertion("edit.paste", &existing, inserted.iter(), 5).is_ok());
        let error = preflight_notes_insertion("edit.paste", &existing, inserted.iter(), 4).unwrap_err().to_string();
        assert!(error.contains("shape count exceeds limit"));
    }
    #[test]
    fn indexed_copy_retains_selection_order_nested_objects_and_bounded_overlap() {
        let child = Shape { id: ShapeId(2), ..Default::default() };
        let root = Shape { id: ShapeId(1), ..group(vec![child]) };
        let owner = vec![root];
        let selected = selected_notes_shapes("edit.copy", &owner, &[ShapeId(2), ShapeId(1)]).unwrap();
        assert_eq!(selected.iter().map(|shape| shape.id).collect::<Vec<_>>(), vec![ShapeId(2), ShapeId(1)]);
        assert!(preflight_notes_insertion("edit.copy", &[], selected.iter().copied(), 3).is_ok());
        assert!(preflight_notes_insertion("edit.copy", &[], selected.iter().copied(), 2).is_err());
    }
}
