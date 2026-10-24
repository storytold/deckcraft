//! Home ▸ New Slide ▸ Reuse Slides: insert slides from another presentation.
//!
//! The slides take the destination's look by default (each one moves to the destination layout
//! with the same name, else the same kind); with `keepFormatting` the source master and its
//! layouts come along. Pictures, media and sounds are copied under fresh ids, and shape ids are
//! renumbered with animations, triggers and connector ends following them. Links to another
//! reused slide follow its copy.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_model::text::{Action, Hyperlink, RunProps, TextBody};
use deckcraft_model::{LayoutId, Master, MasterId, MediaId, Presentation, ShapeId, ShapeKind, Slide, SlideId};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session, media_refs};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "slide.reuseList",
            "List Slides to Reuse",
            [],
            None,
            "{path | data: base64, name?} → {name, slides: [{index, title, hidden}]}: the slides of another presentation, for choosing which ones `slide.reuse` brings in",
            has_doc,
            reuse_list
        ),
        cmd!(
            "slide.reuse",
            "Reuse Slides…",
            ["Home", "Slides", "New Slide"],
            None,
            "{path? | data: base64, name?, slides?: [index] (0-based, default all), keepFormatting?: bool (default false: use the destination theme), at?: index (default after the current slide)}",
            has_doc,
            reuse
        ),
    ]
}

fn reuse_list(_s: &mut Session, p: &Value) -> Result<Value> {
    let (name, bytes) = super::insert::media_bytes(p, "slide.reuseList")?;
    let src = super::file::open_presentation(&name, &bytes)?;
    let slides: Vec<Value> =
        src.slides.iter().enumerate().map(|(index, sl)| json!({"index": index, "title": sl.title(), "hidden": sl.hidden})).collect();
    Ok(json!({"name": name, "slides": slides}))
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

/// Point a link to a reused slide at its copy (`map`: source → destination).
fn relink(h: &mut Option<Hyperlink>, map: &HashMap<SlideId, SlideId>) {
    let Some(h) = h else { return };
    if let Action::Slide { slide } = &mut h.action {
        *slide = map.get(slide).copied().unwrap_or(*slide);
    }
    if let Some(z) = &mut h.zoom_return {
        *z = map.get(z).copied().unwrap_or(*z);
    }
}

fn relink_run(r: &mut RunProps, map: &HashMap<SlideId, SlideId>) {
    relink(&mut r.link, map);
    relink(&mut r.hover, map);
}

fn relink_text(t: &mut TextBody, map: &HashMap<SlideId, SlideId>) {
    for lv in t.list_style.levels.iter_mut().flatten() {
        relink_run(&mut lv.run, map);
    }
    for p in &mut t.paragraphs {
        relink_run(&mut p.def_props, map);
        relink_run(&mut p.end_props, map);
        for r in &mut p.runs {
            relink_run(&mut r.props, map);
        }
    }
}

/// Point the slide links under `shapes` (click and hover actions, text links) at the copies.
fn relink_shapes(shapes: &mut [deckcraft_model::Shape], map: &HashMap<SlideId, SlideId>, depth: usize) {
    if depth > 64 {
        return;
    }
    for sh in shapes {
        relink(&mut sh.click, map);
        relink(&mut sh.hover, map);
        if let Some(t) = &mut sh.text {
            relink_text(t, map);
        }
        if let ShapeKind::Table(t) = &mut sh.kind {
            for c in t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
                relink_text(&mut c.text, map);
            }
        }
        if let Some(ch) = sh.children_mut() {
            relink_shapes(ch, map, depth + 1);
        }
    }
}

/// Fresh shape ids for `shapes`, and the old → new map.
fn renumber(cmd: &str, doc: &mut Presentation, shapes: &mut [deckcraft_model::Shape]) -> Result<HashMap<ShapeId, ShapeId>> {
    let old = shape_ids(shapes);
    super::edit::reid(cmd, doc, shapes)?;
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
        _ => (0..n).collect(),
    };
    if picked.is_empty() {
        return Err(bad("slide.reuse", format!("{name} has no slides to reuse")));
    }
    let keep = bool_or(p, "keepFormatting", false);
    let at = usize_param(p, "at");
    s.edit(|doc, sel| {
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
                let m = import_master("slide.reuse", doc, &src, m, &mut media_map, &mut layout_map)?;
                doc.masters.push(Arc::new(m));
            }
        }
        let mut copies: Vec<Slide> = vec![];
        // Source slide → its copy, so links between reused slides follow (forward ones too).
        let mut slide_map: HashMap<SlideId, SlideId> = HashMap::new();
        for &i in &picked {
            let Some(from) = src.slides.get(i) else { continue };
            copy_media("slide.reuse", doc, &src, &media_refs::slide_media(from), &mut media_map)?;
            let mut sl: Slide = from.as_ref().clone();
            media_refs::visit_mut::slide(&mut sl, &mut remap(&media_map));
            sl.id = SlideId(doc.alloc_id().ok_or_else(|| bad("slide.reuse", NO_ID_SPACE))?);
            let map = renumber("slide.reuse", doc, &mut sl.shapes)?;
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
            slide_map.insert(from.id, sl.id);
            copies.push(sl);
        }
        let mut new_ids = vec![];
        for (k, mut sl) in copies.into_iter().enumerate() {
            relink_shapes(&mut sl.shapes, &slide_map, 0);
            relink_text(&mut sl.notes, &slide_map);
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

/// A copy of `src`'s master `m` ready to join `doc`: its media copied in, fresh master, layout and
/// shape ids. `layouts` gains each source layout id → its copy's.
pub(crate) fn import_master(
    cmd: &str,
    doc: &mut Presentation,
    src: &Presentation,
    m: &Master,
    media_map: &mut HashMap<MediaId, MediaId>,
    layouts: &mut HashMap<LayoutId, LayoutId>,
) -> Result<Master> {
    let mut used = vec![];
    media_refs::visit::master(m, &mut |id: &MediaId| used.push(*id));
    copy_media(cmd, doc, src, &used, media_map)?;
    let mut m = m.clone();
    media_refs::visit_mut::master(&mut m, &mut remap(media_map));
    m.id = MasterId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
    renumber(cmd, doc, &mut m.shapes)?;
    for l in &mut m.layouts {
        let new = LayoutId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
        layouts.insert(l.id, new);
        l.id = new;
        renumber(cmd, doc, &mut l.shapes)?;
    }
    Ok(m)
}

/// Copy the source media `used` into `doc` under fresh ids, once each (identical bytes already in
/// `doc` are shared; a linked item keeps its link).
fn copy_media(cmd: &str, doc: &mut Presentation, src: &Presentation, used: &[MediaId], map: &mut HashMap<MediaId, MediaId>) -> Result<()> {
    for &id in used {
        if map.contains_key(&id) {
            continue;
        }
        let Some(m) = src.media(id) else { continue };
        let new = if m.link.is_some() {
            let mut item = m.clone();
            item.id = MediaId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
            let new = item.id;
            doc.media.push(item);
            new
        } else {
            doc.add_media(&m.name, &m.content_type, m.data.as_ref().clone()).ok_or_else(|| bad(cmd, NO_ID_SPACE))?
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

    fn link_to(slide: SlideId) -> Hyperlink {
        Hyperlink {
            action: Action::Slide { slide },
            tooltip: String::new(),
            highlight_click: false,
            sound: None,
            stop_sound: false,
            zoom_return: None,
        }
    }

    fn linked_text(slide: SlideId) -> TextBody {
        let mut text = TextBody::from_text("go");
        if let Some(r) = text.paragraphs.first_mut().and_then(|p| p.runs.first_mut()) {
            r.props.link = Some(link_to(slide));
        }
        text
    }

    /// Links to a slide that is reused too follow the copy; links to a slide left behind stay.
    #[test]
    fn reused_slides_link_to_each_others_copies() {
        let mut src = defaults::new_presentation(None);
        let template = src.slides.first().map(|s| s.as_ref().clone()).expect("slide");
        for _ in 0..2 {
            let mut sl = template.clone();
            sl.id = SlideId(src.alloc_id().expect("id"));
            src.slides.push(Arc::new(sl));
        }
        let ids: Vec<SlideId> = src.slides.iter().map(|s| s.id).collect();
        let id = |i: usize| ids.get(i).copied().unwrap_or_default();
        if let Some(sl) = src.slides.first_mut().map(Arc::make_mut) {
            sl.shapes = vec![
                Shape { id: ShapeId(9001), click: Some(link_to(id(1))), ..Default::default() },
                Shape { id: ShapeId(9002), text: Some(linked_text(id(1))), ..Default::default() },
                Shape { id: ShapeId(9003), click: Some(link_to(id(2))), ..Default::default() },
            ];
        }
        if let Some(sl) = src.slides.get_mut(1).map(Arc::make_mut) {
            sl.shapes = vec![Shape { id: ShapeId(9004), click: Some(link_to(id(0))), ..Default::default() }];
        }
        let bytes = deckcraft_format::save(&src).expect("save");
        let reopened = crate::cmd::file::open_presentation("src.deckcraft", &bytes).expect("open");
        let left_behind = reopened.slides.get(2).map(|s| s.id).expect("third slide");

        let mut s = Session::with_new();
        for _ in 0..3 {
            s.execute("slide.new", &json!({})).expect("new");
        }
        let out = s
            .execute("slide.reuse", &json!({"data": super::super::base64_encode(&bytes), "name": "src.deckcraft", "slides": [0, 1]}))
            .expect("reuse");
        let new: Vec<SlideId> = serde_json::from_value(out.get("ids").cloned().unwrap_or_default()).expect("ids");
        let doc = &s.doc().expect("doc").doc;
        let copy = |i: usize| new.get(i).and_then(|id| doc.slide(*id)).expect("copied slide");
        let click = |sh: Option<&Shape>| sh.and_then(|sh| sh.click.as_ref()).map(|h| h.action.clone());
        let first = copy(0);
        let to = |i: usize| Some(Action::Slide { slide: new.get(i).copied().unwrap_or_default() });
        assert_eq!(click(first.shapes.first()), to(1), "shape click follows the copy");
        let run_link = first
            .shapes
            .get(1)
            .and_then(|sh| sh.text.as_ref())
            .and_then(|t| t.paragraphs.first())
            .and_then(|p| p.runs.first())
            .and_then(|r| r.props.link.as_ref())
            .map(|h| h.action.clone());
        assert_eq!(run_link, to(1), "text link follows the copy");
        assert_eq!(click(first.shapes.get(2)), Some(Action::Slide { slide: left_behind }), "a slide not reused keeps its link");
        assert_eq!(click(copy(1).shapes.first()), to(0), "a link back to an earlier copy follows too");
    }

    /// The list that lets a dialog offer the slides of the other file; picking from it brings in
    /// exactly those.
    #[test]
    fn reuse_list_names_the_slides_and_the_picked_ones_come_in() {
        let mut src = Session::with_new();
        for t in ["Alpha", "Beta", "Gamma"] {
            src.execute("slide.new", &json!({"layout": "titleAndContent", "title": t})).expect("new");
        }
        let last_slide = src.doc().expect("doc").doc.slides.len() - 1;
        src.execute("slide.hide", &json!({"index": last_slide})).expect("hide");
        let bytes = crate::cmd::file::save_bytes(&src.doc().expect("doc").doc, "deckcraft").expect("save");
        let data = super::super::base64_encode(&bytes);

        let mut s = Session::with_new();
        let listed = s.execute("slide.reuseList", &json!({"data": data, "name": "src.deckcraft"})).expect("list");
        let slides = listed["slides"].as_array().expect("slides");
        let title = |i: usize| slides.iter().find(|x| x["index"] == i).and_then(|x| x["title"].as_str()).unwrap_or_default().to_string();
        assert!(slides.len() >= 3);
        let (a, b, g) = (title(slides.len() - 3), title(slides.len() - 2), title(slides.len() - 1));
        assert_eq!((a.as_str(), b.as_str(), g.as_str()), ("Alpha", "Beta", "Gamma"));
        assert_eq!(slides.last().map(|x| x["hidden"].clone()), Some(json!(true)));
        assert_eq!(s.doc().expect("doc").doc.slides.len(), 1, "listing changes nothing");

        let last = slides.len() - 1;
        s.execute("slide.reuse", &json!({"data": data, "name": "src.deckcraft", "slides": [last - 2, last]})).expect("reuse");
        let titles: Vec<String> = s.doc().expect("doc").doc.slides.iter().map(|x| x.title()).collect();
        assert!(titles.iter().any(|t| t == "Alpha") && titles.iter().any(|t| t == "Gamma"));
        assert!(!titles.iter().any(|t| t == "Beta"), "a slide that was not picked stays out");
    }
}
