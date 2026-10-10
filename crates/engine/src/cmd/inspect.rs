//! Inspection for agents: the deck, slides and shapes as JSON (verify work without screenshots).

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::{Shape, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "document.inspect", "Inspect Presentation", [], None, "{} → slides with titles, layouts, shape counts; selection; size; theme", has_doc, document),
        cmd!(query "slide.inspect", "Inspect Slide", [], None, "{index?} → the slide's shapes with ids, kinds, boxes, text, fills, animations, transition, notes", has_doc, slide),
        cmd!(query "shape.inspect", "Inspect Shape", [], None, "{id} → full shape JSON plus its effective box", has_doc, shape),
        cmd!(query "commands.list", "List Commands", [], None, "{filter?: substring} → [{id, label, params, enabled}]", always, commands),
    ]
}

fn document(s: &mut Session, _p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let d = &st.doc;
    let slides: Vec<Value> = d
        .slides
        .iter()
        .enumerate()
        .map(|(i, sl)| {
            let layout = d.layout(sl.layout).map(|(_, l)| l.name.clone()).unwrap_or_default();
            json!({"index": i, "id": sl.id, "title": sl.title(), "layout": layout, "shapes": sl.shapes.len(), "hidden": sl.hidden, "transition": sl.transition.as_ref().map(|t| t.kind.clone()), "animations": sl.animations.len(), "notes": sl.notes_text()})
        })
        .collect();
    let m = d.masters.first();
    Ok(json!({
        "title": st.title(),
        "path": st.path,
        "dirty": st.is_dirty(),
        "slideSize": [d.slide_size.width, d.slide_size.height],
        "slides": slides,
        "sections": d.sections.iter().map(|x| json!({"name": x.name, "slides": x.slides.len()})).collect::<Vec<_>>(),
        "theme": m.map(|m| m.theme.name.clone()),
        "fonts": m.map(|m| json!({"major": m.theme.fonts.major.latin, "minor": m.theme.fonts.minor.latin})),
        "layouts": m.map(|m| m.layouts.iter().map(|l| json!({"id": l.id, "name": l.name})).collect::<Vec<_>>()),
        "selection": serde_json::to_value(&st.selection).unwrap_or_default(),
        "media": d.media.iter().map(|x| json!({"id": x.id, "name": x.name, "type": x.content_type, "bytes": x.data.len()})).collect::<Vec<_>>(),
        "undo": st.history.undo.last().map(|e| e.label.clone()),
        "redo": st.history.redo.last().map(|e| e.label.clone()),
    }))
}

pub fn shape_summary(ctx: Option<&Ctx>, sh: &Shape, depth: usize) -> Value {
    let x = ctx.map(|c| resolve::xfrm(c, sh)).or(sh.xfrm).unwrap_or_default();
    let fill = ctx.map(|c| {
        let (f, ph) = resolve::fill(c, sh);
        match f {
            Some(deckcraft_model::Fill::Solid { color }) => json!(format!("#{}", c.color(&color, ph).hex())),
            Some(deckcraft_model::Fill::None) | None => json!("none"),
            Some(other) => json!(serde_json::to_value(&other).ok().and_then(|v| v.get("kind").cloned()).unwrap_or_default()),
        }
    });
    let mut v = json!({
        "id": sh.id,
        "name": sh.name,
        "kind": sh.kind_name(),
        "box": [round(x.x), round(x.y), round(x.w), round(x.h)],
        "rotation": x.rot,
    });
    if let Some(o) = v.as_object_mut() {
        if let Some(p) = &sh.ph {
            o.insert("placeholder".into(), json!(p.kind.xml()));
        }
        if let Some(n) = sh.geom.preset_name()
            && matches!(sh.kind, ShapeKind::Shape | ShapeKind::Connector { .. })
        {
            o.insert("preset".into(), json!(n));
        }
        if let Some(t) = &sh.text
            && !t.is_empty()
        {
            o.insert("text".into(), json!(t.text()));
        }
        if let Some(f) = fill {
            o.insert("fill".into(), f);
        }
        if sh.hidden {
            o.insert("hidden".into(), json!(true));
        }
        if !sh.descr.is_empty() {
            o.insert("altText".into(), json!(sh.descr));
        }
        match &sh.kind {
            ShapeKind::Table(t) => {
                o.insert("table".into(), json!({"rows": t.n_rows(), "cols": t.n_cols(), "style": t.style, "cells": t.rows.iter().map(|r| r.cells.iter().map(|c| c.text.text()).collect::<Vec<_>>()).collect::<Vec<_>>()}));
            }
            ShapeKind::Chart(c) => {
                o.insert("chart".into(), json!({"type": c.kind.label(), "title": c.title, "categories": c.categories, "series": c.series.iter().map(|s| json!({"name": s.name, "values": s.values})).collect::<Vec<_>>()}));
            }
            ShapeKind::Group { children, .. } if depth < 8 => {
                o.insert("children".into(), Value::Array(children.iter().map(|c| shape_summary(None, c, depth + 1)).collect()));
            }
            ShapeKind::Media(m) => {
                o.insert("media".into(), json!({"id": m.media, "video": m.video, "autoplay": m.autoplay, "loop": m.loop_play}));
            }
            _ => {}
        }
    }
    v
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn slide(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let i = usize_param(p, "index").unwrap_or(st.selection.slide);
    let sl = st.doc.slides.get(i).ok_or_else(|| bad("slide.inspect", format!("no slide {i}")))?;
    let ctx = Ctx::for_slide(&st.doc, sl);
    let shapes: Vec<Value> = sl.shapes.iter().map(|sh| shape_summary(ctx.as_ref(), sh, 0)).collect();
    let bg = ctx.as_ref().map(|c| match resolve::background(c, Some(sl)) {
        (deckcraft_model::Fill::Solid { color }, ph) => json!(format!("#{}", c.color(&color, ph).hex())),
        (f, _) => serde_json::to_value(&f).ok().and_then(|v| v.get("kind").cloned()).unwrap_or_default(),
    });
    Ok(json!({
        "index": i,
        "id": sl.id,
        "layout": st.doc.layout(sl.layout).map(|(_, l)| l.name.clone()),
        "title": sl.title(),
        "background": bg,
        "shapes": shapes,
        "transition": serde_json::to_value(&sl.transition).unwrap_or_default(),
        "animations": serde_json::to_value(&sl.animations).unwrap_or_default(),
        "notes": sl.notes_text(),
        "comments": sl.comments.len(),
        "hidden": sl.hidden,
    }))
}

fn shape(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let id =
        id_param(p, "id", "shape.inspect")?.or_else(|| st.selection.shapes.first().copied()).ok_or_else(|| bad("shape.inspect", "missing `id`"))?;
    let sh = st.shape(id).ok_or_else(|| bad("shape.inspect", format!("no shape {id}")))?;
    let x = xfrm_of(&st.doc, &st.selection, sh);
    Ok(json!({"shape": serde_json::to_value(sh).unwrap_or_default(), "box": x}))
}

fn commands(s: &mut Session, p: &Value) -> Result<Value> {
    let f = str_param(p, "filter").unwrap_or("").to_ascii_lowercase();
    Ok(Value::Array(
        s.commands()
            .into_iter()
            .filter(|c| f.is_empty() || c.id.to_ascii_lowercase().contains(&f) || c.label.to_ascii_lowercase().contains(&f))
            .map(|c| serde_json::to_value(c).unwrap_or_default())
            .collect(),
    ))
}
