//! Home ▸ New Slide ▸ Reuse Slides: insert slides from another presentation.
//!
//! The slides take the destination's look by default (each one moves to the destination layout
//! with the same name, else the same kind); with `keepFormatting` the source master and its
//! layouts come along. Pictures, media and sounds are copied under fresh ids, and shape ids are
//! renumbered with animations, triggers and connector ends following them.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_model::{LayoutId, MasterId, MediaId, Presentation, ShapeId, ShapeKind, Slide, SlideId};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session, media_refs};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "slide.reuse",
        "Reuse Slides…",
        ["Home", "Slides", "New Slide"],
        None,
        "{path? | data: base64, name?, slides?: [index] (0-based, default all), keepFormatting?: bool (default false: use the destination theme), at?: index (default after the current slide)}",
        has_doc,
        reuse
    )]
}

/// Point every media id under `x` at its copy in the destination (`map`: source → destination).
fn remap(map: &HashMap<MediaId, MediaId>) -> impl FnMut(&mut MediaId) + '_ {
    move |id: &mut MediaId| {
        if let Some(to) = map.get(id) {
            *id = *to;
        }
    }
}

fn shape_ids(shapes: &[deckcraft_model::Shape]) -> Vec<ShapeId> {
    let mut v = vec![];
    deckcraft_model::walk(shapes, &mut |sh, _| v.push(sh.id));
    v
}

/// Point connector ends at the renumbered shapes.
fn remap_connectors(shapes: &mut [deckcraft_model::Shape], map: &HashMap<ShapeId, ShapeId>, depth: usize) {
    if depth > 64 {
        return;
    }
    for sh in shapes {
        if let ShapeKind::Connector { start, end } = &mut sh.kind {
            for c in [start, end] {
                if let Some((id, _)) = c.as_mut() {
                    *id = map.get(id).copied().unwrap_or(*id);
                }
            }
        }
        if let Some(ch) = sh.children_mut() {
            remap_connectors(ch, map, depth + 1);
        }
    }
}

/// Fresh shape ids for `shapes`, and the old → new map.
fn renumber(doc: &mut Presentation, shapes: &mut [deckcraft_model::Shape]) -> Result<HashMap<ShapeId, ShapeId>> {
    let old = shape_ids(shapes);
    super::edit::reid("slide.reuse", doc, shapes)?;
    let map: HashMap<ShapeId, ShapeId> = old.into_iter().zip(shape_ids(shapes)).collect();
    remap_connectors(shapes, &map, 0);
    Ok(map)
}

/// The destination layout a reused slide moves to: same name, else same kind, else the first.
fn matching_layout(doc: &Presentation, name: &str, kind: deckcraft_model::LayoutType) -> LayoutId {
    let all = || doc.masters.iter().flat_map(|m| m.layouts.iter());
    all()
        .find(|l| !name.is_empty() && l.name.eq_ignore_ascii_case(name))
        .or_else(|| all().find(|l| l.kind == kind))
        .or_else(|| all().next())
        .map(|l| l.id)
        .unwrap_or_default()
}

fn reuse(s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = super::insert::media_bytes(p, "slide.reuse")?;
    let src = super::file::open_presentation(&name, &bytes)?;
    let n = src.slides.len();
    let picked: Vec<usize> = match p.get("slides") {
        Some(Value::Array(a)) => {
            if a.len() > crate::transfer_policy::MAX_POLICY_PAGES {
                return Err(bad("slide.reuse", "too many selected pages for transfer"));
            }
            let mut v = vec![];
            for x in a {
                let i = x.as_u64().map(|i| i as usize).filter(|i| *i < n);
                let i = i.ok_or_else(|| bad("slide.reuse", format!("{name} has {n} slides; {x} isn't one of them (indices count from 0)")))?;
                if !v.contains(&i) {
                    v.push(i);
                }
            }
            v
        }
        Some(Value::String(t)) if !t.trim().is_empty() => super::file::parse_slide_range(t, n)?,
        _ => {
            if n > crate::transfer_policy::MAX_POLICY_PAGES {
                return Err(bad("slide.reuse", "too many selected pages for transfer"));
            }
            (0..n).collect()
        }
    };
    if picked.is_empty() {
        return Err(bad("slide.reuse", format!("{name} has no slides to reuse")));
    }
    let keep = bool_or(p, "keepFormatting", false);
    let at = usize_param(p, "at");
    if picked.len() > crate::transfer_policy::MAX_POLICY_PAGES {
        return Err(bad("slide.reuse", "too many selected pages for transfer"));
    }
    let selected: Vec<_> = picked.iter().filter_map(|index| src.slides.get(*index).map(Arc::as_ref)).collect();
    deckcraft_model::comments::validate(&src, true).map_err(|e| bad("slide.reuse", e.to_string()))?;
    let mut prepared: Vec<Slide> = selected.iter().map(|s| (*s).clone()).collect();
    crate::comment_transfer::capture(&src, &mut prepared)?;
    let selected: Vec<&Slide> = prepared.iter().collect();
    let notes_capture = crate::notes_transfer::capture(&src, &selected)?;
    if let Some(reason) = notes_capture.same_document_only {
        return Err(bad("slide.reuse", format!("native notes transfer refused: {reason}")));
    }
    let notes = notes_capture.closure;
    if notes.is_none() {
        crate::transfer_policy::check_external(&src, selected.iter().copied())?;
    }
    if let Some(notes) = &notes {
        crate::notes_transfer::preflight(&s.doc()?.doc, notes, selected.iter().copied())?;
    }
    let templates = crate::template_transfer::capture(&src, picked.iter().filter_map(|index| src.slides.get(*index).map(Arc::as_ref)))?;
    let mut slides: Vec<Slide> = prepared;
    if let Some(notes) = &notes {
        return s.edit_resources(|doc, sel| {
            crate::notes_transfer::remap(doc, notes, &mut slides, keep)?;
            crate::template_transfer::remap(doc, &templates, &mut slides)?;
            let at = at.unwrap_or(sel.slide + 1).min(doc.slides.len());
            let mut ids = Vec::new();
            ids.try_reserve_exact(slides.len()).map_err(|_| bad("slide.reuse", "cannot allocate inserted slide index"))?;
            doc.slides.try_reserve(slides.len()).map_err(|_| bad("slide.reuse", "cannot allocate inserted slides"))?;
            for (offset, slide) in slides.into_iter().enumerate() {
                ids.push(slide.id);
                doc.slides.insert((at + offset).min(doc.slides.len()), Arc::new(slide));
            }
            sel.slide = (at + ids.len()).saturating_sub(1).min(doc.slides.len().saturating_sub(1));
            sel.slides = ids.clone();
            sel.shapes.clear();
            sel.text = None;
            Ok(json!({"slides":ids.len(), "ids":ids, "from":name, "keepFormatting":keep}))
        });
    }
    s.edit_resources(|doc, sel| {
        deckcraft_model::comments::normalize(doc).map_err(|e| bad("slide.reuse", e.to_string()))?;
        crate::template_transfer::remap(doc, &templates, &mut slides)?;
        let at = at.unwrap_or(sel.slide + 1).min(doc.slides.len());
        // Media: what the slides use is copied under fresh ids.
        let mut media_map: HashMap<MediaId, MediaId> = HashMap::new();
        // Keep Source Formatting: bring each source master used, once, with fresh ids.
        let mut layout_map: HashMap<LayoutId, LayoutId> = HashMap::new();
        if keep {
            let mut masters_done: Vec<MasterId> = vec![];
            for &i in &picked {
                let Some((m, _)) = src.slides.get(i).and_then(|sl| src.master_for(sl)) else { continue };
                if masters_done.contains(&m.id) {
                    continue;
                }
                masters_done.push(m.id);
                let mut used = vec![];
                media_refs::visit::master(m, &mut |id: &MediaId| used.push(*id));
                copy_media(doc, &src, &used, &mut media_map)?;
                let mut m = m.clone();
                media_refs::visit_mut::master(&mut m, &mut remap(&media_map));
                m.id = MasterId(doc.alloc_id().ok_or_else(|| bad("slide.reuse", NO_ID_SPACE))?);
                renumber(doc, &mut m.shapes)?;
                for l in &mut m.layouts {
                    let new = LayoutId(doc.alloc_id().ok_or_else(|| bad("slide.reuse", NO_ID_SPACE))?);
                    layout_map.insert(l.id, new);
                    l.id = new;
                    renumber(doc, &mut l.shapes)?;
                }
                doc.masters.push(Arc::new(m));
            }
        }
        let mut new_ids = vec![];
        let mut comment_context = crate::comment_transfer::Context::default();
        let mut comment_slides = std::collections::BTreeMap::new();
        for slide in &mut slides {
            let old = slide.id;
            slide.id = SlideId(doc.alloc_id().ok_or_else(|| bad("slide.reuse", NO_ID_SPACE))?);
            comment_slides.insert(old, slide.id);
        }
        for (k, mut sl) in slides.into_iter().enumerate() {
            let i = *picked.get(k).ok_or_else(|| bad("slide.reuse", "source slide index disappeared"))?;
            let Some(from) = src.slides.get(i) else { continue };
            copy_media(doc, &src, &media_refs::slide_media(from), &mut media_map)?;
            let mut comments = std::mem::take(&mut sl.comments);
            media_refs::visit_mut::slide(&mut sl, &mut remap(&media_map));
            let map = renumber(doc, &mut sl.shapes)?;
            let comment_shapes = map.iter().map(|(a, b)| (*a, *b)).collect();
            let comment_media = crate::comment_transfer::identity_media(&comments, &media_map);
            crate::comment_transfer::remap(doc, &mut comments, &comment_shapes, &comment_media, &comment_slides, &mut comment_context)?;
            sl.comments = comments;
            for a in &mut sl.animations {
                a.shape = map.get(&a.shape).copied().unwrap_or(a.shape);
                a.trigger = a.trigger.map(|t| map.get(&t).copied().unwrap_or(t));
            }
            sl.layout = match layout_map.get(&sl.layout) {
                Some(l) => *l,
                None => {
                    let (lname, kind) =
                        src.layout(from.layout).map(|(_, l)| (l.name.clone(), l.kind)).unwrap_or_else(|| (String::new(), Default::default()));
                    matching_layout(doc, &lname, kind)
                }
            };
            new_ids.push(sl.id);
            doc.slides.insert((at + k).min(doc.slides.len()), Arc::new(sl));
        }
        sel.slide = (at + new_ids.len()).saturating_sub(1).min(doc.slides.len().saturating_sub(1));
        sel.slides = new_ids.clone();
        sel.shapes.clear();
        sel.text = None;
        Ok(json!({"slides": new_ids.len(), "ids": new_ids, "from": name, "keepFormatting": keep}))
    })
}

/// Copy the source media `used` into `doc` under fresh ids, once each (identical bytes already in
/// `doc` are shared; a linked item keeps its link).
fn copy_media(doc: &mut Presentation, src: &Presentation, used: &[MediaId], map: &mut HashMap<MediaId, MediaId>) -> Result<()> {
    for &id in used {
        if map.contains_key(&id) {
            continue;
        }
        let Some(m) = src.media(id) else { continue };
        let new = if m.link.is_some() {
            let mut item = m.clone();
            item.id = MediaId(doc.alloc_id().ok_or_else(|| bad("slide.reuse", NO_ID_SPACE))?);
            let new = item.id;
            doc.media.push(item);
            new
        } else {
            doc.add_media(&m.name, &m.content_type, m.data.as_ref().clone()).ok_or_else(|| bad("slide.reuse", NO_ID_SPACE))?
        };
        map.insert(id, new);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use deckcraft_model::style::{Artistic, Fill, PictureFill};
    use deckcraft_model::text::{Action, Bullet, Hyperlink, TextBody};
    use deckcraft_model::{Background, MediaClip, Shape, ShapeKind, defaults};
    use serde_json::json;

    use super::*;

    /// Every kind of media a slide can hold comes along, each pointing at its own bytes: the
    /// artistic-effect original, a background picture, poster, sounds and picture bullets too.
    #[test]
    fn reused_slides_bring_every_media_reference() {
        let mut src = defaults::new_presentation(None);
        let blobs: Vec<Vec<u8>> = (0..8u8).map(|i| vec![0xD0, i, i, i]).collect();
        let ids: Vec<MediaId> = blobs
            .iter()
            .enumerate()
            .map(|(i, b)| src.add_media(&format!("m{i}.bin"), "application/octet-stream", b.clone()).expect("media"))
            .collect();
        let id = |i: usize| ids.get(i).copied().unwrap_or_default();
        let mut text = TextBody::from_text("bullet");
        if let Some(p) = text.paragraphs.first_mut() {
            p.props.bullet = Some(Bullet::Picture { media: id(6) });
        }
        let slide = Arc::make_mut(src.slides.first_mut().expect("slide"));
        slide.shapes = vec![
            Shape {
                id: ShapeId(9001),
                kind: ShapeKind::Picture {
                    fill: PictureFill {
                        media: id(0),
                        artistic: Some(Artistic { effect: "artisticBlur".into(), original: id(1) }),
                        ..Default::default()
                    },
                },
                ..Default::default()
            },
            Shape {
                id: ShapeId(9002),
                kind: ShapeKind::Media(MediaClip { media: id(2), poster: Some(id(3)), ..Default::default() }),
                ..Default::default()
            },
            Shape {
                id: ShapeId(9003),
                click: Some(Hyperlink {
                    action: Action::NextSlide,
                    tooltip: String::new(),
                    highlight_click: false,
                    sound: Some(id(5)),
                    stop_sound: false,
                    zoom_return: None,
                    sound_name: None,
                }),
                text: Some(text),
                ..Default::default()
            },
        ];
        slide.background = Some(Background::Fill { fill: Fill::Picture(PictureFill { media: id(4), ..Default::default() }) });
        slide.transition = Some(deckcraft_model::anim::Transition { sound: Some(id(7)), ..Default::default() });
        let bytes = deckcraft_format::save(&src).expect("save");

        let mut s = Session::with_new();
        let own = vec![1u8, 2, 3];
        let own_id = s.edit(|doc, _| Ok(doc.add_media("own.bin", "application/octet-stream", own.clone()).expect("media"))).expect("own media");
        s.execute("slide.reuse", &json!({"data": super::super::base64_encode(&bytes), "name": "src.deckcraft", "slides": [0]})).expect("reuse");
        let doc = &s.doc().expect("doc").doc;
        assert_eq!(doc.media(own_id).map(|m| m.data.as_slice() == own.as_slice()), Some(true), "own media untouched");
        let reused = doc.slides.get(1).expect("reused slide");
        let got: Vec<Vec<u8>> =
            media_refs::slide_media(reused).iter().map(|m| doc.media(*m).map(|m| m.data.as_ref().clone()).unwrap_or_default()).collect();
        let mut got = got;
        got.sort();
        let mut want = blobs.clone();
        want.sort();
        assert_eq!(got, want, "every reference resolves to its own copied bytes");
    }
}
