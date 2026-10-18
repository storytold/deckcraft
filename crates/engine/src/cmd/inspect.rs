//! Inspection for agents: the deck, slides and shapes as JSON (verify work without screenshots).

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::Hyperlink;
use deckcraft_model::{Shape, ShapeKind};
use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd, has_doc, id_param, opt_usize_param, str_param, usize_param, xfrm_of};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "document.inspect", "Inspect Presentation", [], None, "{} → slides with titles, layouts, shape counts; selection; size; theme", has_doc, document),
        cmd!(query "slide.inspect", "Inspect Slide", [], None, "{index?, animations?: true (the full list; default a summary {count, clickSteps}, animation.get has the list)} → the slide's shapes with ids, kinds, boxes, text, fills, and when present textColor (a scheme colour as its slot name, else hex), line {color, width}, a connector's from/to {id, site}, links [{text, url|slide}]; animations, transition, notes (text, and notesBody with its formatting)", has_doc, slide),
        cmd!(query "slide.actions", "Inspect Slide Actions", [], None, "{index?} → {index, slideId, actions: [{owner, event, type, target, hyperlink}]}; lists shape click/hover actions and text-run links/hover actions (including table cells) without executing them", has_doc, actions),
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

fn actions(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let i = opt_usize_param(p, "index", "slide.actions")?.unwrap_or(st.selection.slide);
    let sl = st.doc.slides.get(i).ok_or_else(|| bad("slide.actions", format!("no slide {i}")))?;
    let mut found = Vec::new();
    for sh in &sl.shapes {
        collect_actions(sh, &mut Vec::new(), &mut found);
    }
    Ok(json!({"index": i, "slideId": sl.id, "actions": found}))
}

fn collect_actions(sh: &Shape, path: &mut Vec<Value>, found: &mut Vec<Value>) {
    path.push(json!({"id": sh.id, "name": sh.name}));
    let owner = json!({"shapeId": sh.id, "shapeName": sh.name, "path": path});
    if let Some(link) = &sh.click {
        found.push(action_entry(owner.clone(), "click", link));
    }
    if let Some(link) = &sh.hover {
        found.push(action_entry(owner.clone(), "hover", link));
    }
    if let Some(text) = &sh.text {
        collect_text_actions(text, &owner, found);
    }
    if let ShapeKind::Table(table) = &sh.kind {
        for (row_index, row) in table.rows.iter().enumerate() {
            for (cell_index, cell) in row.cells.iter().enumerate() {
                let mut cell_owner = owner.clone();
                cell_owner["tableRowIndex"] = json!(row_index);
                cell_owner["tableCellIndex"] = json!(cell_index);
                collect_text_actions(&cell.text, &cell_owner, found);
            }
        }
    }
    if let ShapeKind::Group { children, .. } = &sh.kind {
        for child in children {
            collect_actions(child, path, found);
        }
    }
    path.pop();
}

fn collect_text_actions(text: &deckcraft_model::text::TextBody, owner: &Value, found: &mut Vec<Value>) {
    for (paragraph_index, paragraph) in text.paragraphs.iter().enumerate() {
        for (run_index, run) in paragraph.runs.iter().enumerate() {
            for (event, link) in [("textLink", run.props.link.as_ref()), ("textHover", run.props.hover.as_ref())] {
                if let Some(link) = link {
                    let mut text_owner = owner.clone();
                    text_owner["paragraphIndex"] = json!(paragraph_index);
                    text_owner["runIndex"] = json!(run_index);
                    text_owner["text"] = json!(run.content());
                    found.push(action_entry(text_owner, event, link));
                }
            }
        }
    }
}

fn action_entry(owner: Value, event: &str, link: &Hyperlink) -> Value {
    let action = serde_json::to_value(&link.action).unwrap_or_default();
    let kind = action.get("kind").cloned().unwrap_or(Value::Null);
    let target = ["url", "slide", "name", "path"].iter().find_map(|key| action.get(key).cloned()).unwrap_or(Value::Null);
    json!({"owner": owner, "event": event, "type": kind, "target": target, "hyperlink": link})
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

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_geom::Xfrm;
    use deckcraft_model::table::Row;
    use deckcraft_model::text::{Action, Paragraph, Run, RunProps, TextBody};
    use deckcraft_model::{Cell, Shape, ShapeId, ShapeKind, Table};

    fn link(action: Action) -> Hyperlink {
        Hyperlink { action, tooltip: String::new(), highlight_click: false, sound: None, stop_sound: false, zoom_return: None }
    }

    #[test]
    fn slide_actions_lists_group_shape_and_text_actions() {
        let mut s = Session::with_new();
        let mut child = Shape { id: ShapeId(12), name: "Child".into(), ..Default::default() };
        child.click = Some(link(Action::NextSlide));
        child.hover = Some(link(Action::Url { url: "https://example.com/hover".into() }));
        let run_props = RunProps { link: Some(link(Action::Slide { slide: deckcraft_model::SlideId(7) })), ..Default::default() };
        let mut paragraph = Paragraph::new("linked");
        paragraph.runs = vec![Run::with("linked", run_props)];
        child.text = Some(TextBody { paragraphs: vec![paragraph], ..Default::default() });
        let group = Shape {
            id: ShapeId(11),
            name: "Group".into(),
            kind: ShapeKind::Group { children: vec![child], child: Xfrm::default() },
            ..Default::default()
        };
        let st = s.doc_mut().unwrap();
        let slide_index = st.selection.slide;
        let presentation = std::sync::Arc::make_mut(&mut st.doc);
        std::sync::Arc::make_mut(&mut presentation.slides[slide_index]).shapes = vec![group];

        let v = s.execute("slide.actions", &json!({})).unwrap();
        let actions = v["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 3, "{v}");
        assert_eq!(actions[0]["event"], "click");
        assert_eq!(actions[0]["type"], "nextSlide");
        assert_eq!(actions[0]["owner"]["path"][0]["name"], "Group");
        assert_eq!(actions[0]["owner"]["path"][1]["id"], 12);
        assert_eq!(actions[1]["event"], "hover");
        assert_eq!(actions[1]["target"], "https://example.com/hover");
        assert_eq!(actions[2]["event"], "textLink");
        assert_eq!(actions[2]["owner"]["runIndex"], 0);
        assert_eq!(actions[2]["target"], 7);

        let cell_props = RunProps { hover: Some(link(Action::Url { url: "https://example.com/cell".into() })), ..Default::default() };
        let mut cell_text = TextBody::from_text("cell link");
        cell_text.paragraphs[0].runs = vec![Run::with("cell link", cell_props)];
        let table =
            Table { rows: vec![Row { cells: vec![Cell { text: cell_text, ..Default::default() }], ..Default::default() }], ..Default::default() };
        let table_shape = Shape { id: ShapeId(13), name: "Table".into(), kind: ShapeKind::Table(table), ..Default::default() };
        let st = s.doc_mut().unwrap();
        let presentation = std::sync::Arc::make_mut(&mut st.doc);
        std::sync::Arc::make_mut(&mut presentation.slides[slide_index]).shapes.push(table_shape);
        let v = s.execute("slide.actions", &json!({})).unwrap();
        let table_action = v["actions"].as_array().unwrap().last().cloned().unwrap();
        assert_eq!(table_action["event"], "textHover");
        assert_eq!(table_action["owner"]["tableRowIndex"], 0);
        assert_eq!(table_action["owner"]["tableCellIndex"], 0);
        assert_eq!(table_action["target"], "https://example.com/cell");
    }

    #[test]
    fn slide_actions_returns_empty_array_for_slide_without_actions() {
        let mut s = Session::with_new();
        let v = s.execute("slide.actions", &json!({})).unwrap();
        assert_eq!(v["actions"], json!([]));
    }

    #[test]
    fn slide_actions_refuses_a_bad_index_instead_of_using_the_selection() {
        let mut s = Session::with_new();
        for index in [json!(-1), json!("0"), json!(1.5)] {
            assert!(s.execute("slide.actions", &json!({"index": index})).is_err(), "{index}");
        }
    }
}
