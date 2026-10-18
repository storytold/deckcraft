//! Inspection for agents: the deck, slides and shapes as JSON (verify work without screenshots).

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::{Shape, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "document.inspect", "Inspect Presentation", [], None, "{} → slides with titles, layouts, shape counts; selection; size; theme", has_doc, document),
        cmd!(query "slide.inspect", "Inspect Slide", [], None, "{index?, animations?: true (the full list; default a summary {count, clickSteps}, animation.get has the list)} → the slide's shapes with ids, kinds, boxes, text, fills, and when present textColor (a scheme colour as its slot name, else hex), line {color, width}, a connector's from/to {id, site}, links [{text, url|slide}]; animations, transition, notes (text, and notesBody with its formatting)", has_doc, slide),
        cmd!(query "shape.inspect", "Inspect Shape", [], None, "{id} → {shape: full shape JSON, box: its effective box, slide: the index of the slide it is on} (any slide; moves nothing)", has_doc, shape),
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
    let mut x = ctx.map(|c| resolve::xfrm(c, sh)).or(sh.xfrm).unwrap_or_default();
    // A table's rows grow with their text: the box is what draws, not what was asked for.
    if let (Some(c), deckcraft_model::ShapeKind::Table(t)) = (ctx, &sh.kind) {
        x.h = x.h.max(deckcraft_render::table_row_heights(c, t, &deckcraft_text::NoFields).iter().sum());
    }
    // Text that needs more height than its box has: what an agent otherwise learns only from a
    // render. Only for a box that neither grows (autofit shape) nor shrinks its text.
    let overflow = ctx.and_then(|c| {
        let body = sh.text.as_ref().filter(|b| !b.is_empty())?;
        if matches!(sh.kind, deckcraft_model::ShapeKind::Table(_))
            || !matches!(resolve::body(c, sh).autofit, None | Some(deckcraft_model::text::AutoFit::None))
        {
            return None;
        }
        let rect = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
        let need = deckcraft_text::fit_height(c, sh, body, rect, &deckcraft_text::NoFields);
        (need > rect.height() + 0.5).then_some(need)
    });
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
        if let Some(need) = overflow {
            o.insert("textOverflows".into(), json!(true));
            o.insert("textHeight".into(), json!(round(need)));
        }
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
        // What used to cost a render: the text colour, the line, a connector's ends and link
        // targets, each only when present, so a plain shape grows by its text colour alone.
        if let Some(c) = ctx {
            if let Some(tc) = text_color(c, sh) {
                o.insert("textColor".into(), tc);
            }
            if let (deckcraft_model::Fill::Solid { color }, ph) = {
                let (l, ph) = resolve::line(c, sh);
                (l.fill.clone().unwrap_or(deckcraft_model::Fill::None), (l, ph))
            } {
                let mut line = json!({"color": color_json(c, &color, ph.1)});
                if let Some(w) = ph.0.width {
                    line["width"] = json!(w);
                }
                o.insert("line".into(), line);
            }
            let links = links(c, sh);
            if !links.is_empty() {
                o.insert("links".into(), Value::Array(links));
            }
        }
        if let ShapeKind::Connector { start, end } = &sh.kind {
            if let Some((id, site)) = start {
                o.insert("from".into(), json!({"id": id, "site": site}));
            }
            if let Some((id, site)) = end {
                o.insert("to".into(), json!({"id": id, "site": site}));
            }
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

/// A colour as an agent reads it: the theme slot's name for an unmodified scheme colour (so a
/// theme change is seen as such), else the resolved `#RRGGBB`.
fn color_json(c: &Ctx, color: &deckcraft_model::ColorRef, ph: Option<deckcraft_color::Rgba>) -> Value {
    match &color.base {
        deckcraft_model::ColorBase::Scheme { slot } if color.mods.is_empty() => json!(slot.xml_name()),
        _ => json!(format!("#{}", c.color(color, ph).hex())),
    }
}

/// The resolved colour of the shape's first text run, when it has text.
fn text_color(c: &Ctx, sh: &Shape) -> Option<Value> {
    let body = sh.text.as_ref().filter(|t| !t.is_empty())?;
    let para = body.paragraphs.iter().find(|p| !p.runs.is_empty())?;
    let run = para.runs.first()?;
    let r = resolve::run(c, sh, para, &run.props);
    Some(match &r.fill {
        Some(deckcraft_model::Fill::Solid { color }) => color_json(c, color, None),
        _ => json!(format!("#{}", resolve::text_color(c, &r).hex())),
    })
}

/// The shape's hyperlinks: each linked run's text with its target, a `url` or the 0-based
/// `slide` index.
fn links(c: &Ctx, sh: &Shape) -> Vec<Value> {
    use deckcraft_model::text::Action;
    let mut out = vec![];
    let target = |a: &Action| match a {
        Action::Url { url } => Some(("url", json!(url))),
        Action::Slide { slide } => c.pres.slide_index(*slide).map(|i| ("slide", json!(i))),
        _ => None,
    };
    for run in sh.text.iter().flat_map(|t| t.paragraphs.iter()).flat_map(|p| p.runs.iter()) {
        if let Some(link) = &run.props.link
            && let Some((k, v)) = target(&link.action)
        {
            out.push(json!({"text": run.text, k: v}));
        }
    }
    out
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
    // The full list costs 10-15 KB on an animated slide, and agents inspect a slide many times
    // per run; the summary is what they need between edits, animation.get has the list.
    let animations = if bool_param(p, "animations").unwrap_or(false) {
        serde_json::to_value(&sl.animations).unwrap_or_default()
    } else {
        json!({"count": sl.animations.len(), "clickSteps": super::animation::click_steps(&sl.animations)})
    };
    let mut v = json!({
        "index": i,
        "id": sl.id,
        "layout": st.doc.layout(sl.layout).map(|(_, l)| l.name.clone()),
        "title": sl.title(),
        "background": bg,
        "shapes": shapes,
        "transition": serde_json::to_value(&sl.transition).unwrap_or_default(),
        "animations": animations,
        "notes": sl.notes_text(),
        "notesBody": sl.notes,
        "comments": sl.comments.len(),
        "hidden": sl.hidden,
    });
    // Empty notes are "", not a formatted body full of nulls.
    if sl.notes_text().is_empty()
        && let Some(o) = v.as_object_mut()
    {
        o.remove("notesBody");
    }
    prune_nulls(&mut v);
    Ok(v)
}

/// Drops every null-valued key, recursively: an absent field reads the same and costs nothing,
/// where a formatted body or an animation serialises dozens of `null`s.
fn prune_nulls(v: &mut Value) {
    match v {
        Value::Object(o) => {
            o.retain(|_, x| !x.is_null());
            o.values_mut().for_each(prune_nulls);
        }
        Value::Array(a) => a.iter_mut().for_each(prune_nulls),
        _ => {}
    }
}

fn shape(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let id =
        id_param(p, "id", "shape.inspect")?.or_else(|| st.selection.shapes.first().copied()).ok_or_else(|| bad("shape.inspect", "missing `id`"))?;
    // Ids are unique in the presentation: a read-only lookup finds the shape on any slide
    // without making that slide current, and says which slide it is on.
    let (sh, ctx) = st.shape_anywhere(id).ok_or_else(|| super::no_such_shape(&st.doc, &st.selection, id, "shape.inspect"))?;
    let x = xfrm_of(&st.doc, &ctx, sh);
    let mut v = json!({"shape": serde_json::to_value(sh).unwrap_or_default(), "box": x});
    if let Some(i) = slide_of(&ctx) {
        v["slide"] = json!(i);
    }
    Ok(v)
}

/// The slide index of a lookup's context when the shape is on a slide (not a master or layout).
pub(crate) fn slide_of(ctx: &crate::Selection) -> Option<usize> {
    matches!(ctx.target, crate::Target::Slides).then_some(ctx.slide)
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
