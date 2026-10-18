//! Placeholders across the deck. A slide's placeholder takes its position and size from its
//! layout's until it's moved, when it keeps its own (PPTX `a:xfrm` on the slide), so a title
//! nudged on one slide drifts from the rest. These commands find the moved ones, put them back
//! on the layout's position, or make one slide's position the layout's for every slide. A text
//! box can also be made one of its layout's placeholders (imported decks often have titles that
//! are plain text boxes), or a placeholder a plain shape again.

use std::sync::Arc;

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::{PhType, Placeholder, Presentation, Shape, ShapeId, ShapeKind, Xfrm};
use serde_json::{Value, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "placeholder.moved",
            "Moved Placeholders",
            [],
            None,
            "{kind?: title|subTitle|body|dt|ftr|sldNum|… (a title and a title slide's title count as one)} → [{slide: index, id, kind}]: placeholders placed off their layout's position or size",
            has_doc,
            moved
        ),
        cmd!(
            "placeholder.reset",
            "Reset Placeholder Positions",
            [],
            None,
            "{ids?: [placeholders on the current slide] | kind?: (default: the selected placeholder's), slides?: [index] (default: every slide)} → {reset}: those placeholders take their layout's position and size again (text and formatting stay)",
            has_doc,
            reset
        ),
        cmd!(
            "placeholder.toLayout",
            "Use Position on All Slides",
            [],
            None,
            "{id?: a placeholder on the current slide (default: the selected one)} → {reset}: its position and size become its layout placeholder's, and every slide with that layout follows",
            has_slide,
            to_layout
        ),
        cmd!(
            "placeholder.assign",
            "Make Placeholder",
            [],
            None,
            "{id?: a shape on the current slide/layout/master (default: the selected one), kind: title|subTitle|body|obj|dt|ftr|sldNum|…|none} → {id, kind, untagged? | removed?}: the shape becomes a placeholder keeping its position, text and own formatting. On a slide, one its layout has (the shape holding that place is untagged, or deleted when empty); on a layout or the master, a new one of that kind (a title, subtitle, date, footer or slide number already there gives up its place). `none` makes a placeholder a plain shape that looks the same",
            has_doc,
            assign
        ),
    ]
}

/// Title and centred title are one kind here: the same job on different layouts.
fn same_kind(a: PhType, b: PhType) -> bool {
    a == b || (a.is_title() && b.is_title())
}

fn kind_param(s: &Session, p: &Value, cmd: &str) -> Result<PhType> {
    if let Some(k) = str_param(p, "kind") {
        let t = PhType::from_xml(k);
        if t.xml() != k {
            return Err(bad(cmd, format!("unknown placeholder kind `{k}`")));
        }
        return Ok(t);
    }
    let st = s.doc()?;
    st.selected_shapes().iter().find_map(|sh| sh.ph_type()).ok_or_else(|| bad(cmd, "give `kind` or select a placeholder"))
}

/// The position a placeholder inherits from its layout (or master), if it inherits one.
fn inherited(ctx: &Ctx, sh: &Shape) -> Option<Xfrm> {
    let (lp, mp) = resolve::parents(ctx, sh);
    lp.and_then(|s| s.xfrm).or_else(|| mp.and_then(|s| s.xfrm))
}

fn differs(a: &Xfrm, b: &Xfrm) -> bool {
    [a.x - b.x, a.y - b.y, a.w - b.w, a.h - b.h].iter().any(|d| d.abs() > 0.5) || (a.rot - b.rot).abs() > 0.01
}

/// `(slide, shape, kind)` for placeholders of `kind` (any, when None) on `slides` that have a
/// position of their own and a layout or master one to go back to; `only_moved` keeps the ones
/// whose own position differs from it.
fn placed(doc: &Presentation, slides: &[usize], kind: Option<PhType>, only_moved: bool) -> Vec<(usize, ShapeId, PhType)> {
    let mut out = vec![];
    for &i in slides {
        let Some(sl) = doc.slides.get(i) else { continue };
        let Some(ctx) = Ctx::for_slide(doc, sl) else { continue };
        for sh in &sl.shapes {
            let (Some(t), Some(own)) = (sh.ph_type(), sh.xfrm.as_ref()) else { continue };
            if kind.is_some_and(|k| !same_kind(k, t)) {
                continue;
            }
            let Some(from) = inherited(&ctx, sh) else { continue };
            if !only_moved || differs(own, &from) {
                out.push((i, sh.id, t));
            }
        }
    }
    out
}

fn moved(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = match str_param(p, "kind") {
        Some(_) => Some(kind_param(s, p, "placeholder.moved")?),
        None => None,
    };
    let doc = &s.doc()?.doc;
    let all: Vec<usize> = (0..doc.slides.len()).collect();
    let v: Vec<Value> = placed(doc, &all, kind, true).into_iter().map(|(i, id, t)| json!({"slide": i, "id": id, "kind": t.xml()})).collect();
    Ok(Value::Array(v))
}

/// Clear the own position of `targets` (slide, shape) so they follow their layout again.
fn clear(doc: &mut Presentation, targets: &[(usize, ShapeId, PhType)]) {
    for (i, id, _) in targets {
        if let Some(sl) = doc.slides.get_mut(*i)
            && let Some(sh) = Arc::make_mut(sl).shapes.iter_mut().find(|sh| sh.id == *id)
        {
            sh.xfrm = None;
        }
    }
}

fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    // Just these placeholders on the current slide.
    if let Some(ids) = ids_param(p, "ids") {
        let st = s.doc()?;
        let targets: Vec<_> = placed(&st.doc, &[st.selection.slide], None, false).into_iter().filter(|t| ids.contains(&t.1)).collect();
        if targets.is_empty() {
            return Ok(json!({"reset": 0}));
        }
        return s.edit(|doc, _| {
            clear(doc, &targets);
            Ok(json!({"reset": targets.len()}))
        });
    }
    let kind = kind_param(s, p, "placeholder.reset")?;
    let st = s.doc()?;
    let slides: Vec<usize> = match p.get("slides").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_u64).map(|i| i as usize).collect(),
        None => (0..st.doc.slides.len()).collect(),
    };
    let targets = placed(&st.doc, &slides, Some(kind), false);
    if targets.is_empty() {
        return Ok(json!({"reset": 0}));
    }
    s.edit(|doc, _| {
        clear(doc, &targets);
        Ok(json!({"reset": targets.len()}))
    })
}

fn to_layout(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "placeholder.toLayout";
    let st = s.doc()?;
    let doc = &st.doc;
    let id = match id_param(p, "id", CMD)? {
        Some(id) => id,
        None => st.selected_shapes().iter().find(|sh| sh.ph.is_some()).map(|sh| sh.id).ok_or_else(|| bad(CMD, "select a placeholder"))?,
    };
    let slide = doc.slides.get(st.selection.slide).ok_or_else(|| bad(CMD, "no slide"))?;
    let sh = slide.shapes.iter().find(|sh| sh.id == id && sh.ph.is_some()).ok_or_else(|| bad(CMD, "not a placeholder on this slide"))?;
    let ctx = Ctx::for_slide(doc, slide).ok_or_else(|| bad(CMD, "no layout"))?;
    let lp = resolve::parents(&ctx, sh).0.ok_or_else(|| bad(CMD, "this placeholder has no place on its layout"))?;
    let (layout, target, x) = (slide.layout, lp.id, resolve::xfrm(&ctx, sh));
    // Every placeholder that follows the same layout placeholder.
    let mut followers = vec![];
    for (i, sl) in doc.slides.iter().enumerate().filter(|(_, sl)| sl.layout == layout) {
        let Some(ctx) = Ctx::for_slide(doc, sl) else { continue };
        for s in sl.shapes.iter().filter(|s| s.ph.is_some() && s.xfrm.is_some()) {
            if resolve::parents(&ctx, s).0.is_some_and(|l| l.id == target) {
                followers.push((i, s.id, s.ph_type().unwrap_or(PhType::Body)));
            }
        }
    }
    s.edit(|doc, _| {
        let lsh = doc
            .masters
            .iter_mut()
            .find_map(|m| {
                let has = m.layouts.iter().any(|l| l.id == layout);
                has.then(|| Arc::make_mut(m)).and_then(|m| m.layouts.iter_mut().find(|l| l.id == layout))
            })
            .and_then(|l| l.shapes.iter_mut().find(|s| s.id == target))
            .ok_or_else(|| bad(CMD, "the layout placeholder is gone"))?;
        lsh.xfrm = Some(x);
        clear(doc, &followers);
        Ok(json!({"reset": followers.len()}))
    })
}

/// Does a layout placeholder of kind `have` take a shape asked to be `want`?
fn fits(want: PhType, have: PhType) -> bool {
    same_kind(want, have) || (want == PhType::Body && have == PhType::Obj)
}

/// `sh` as a plain shape that looks as it did as a placeholder: what it inherited (position,
/// fill, text box settings, paragraph and character formatting) written onto it.
fn bake(ctx: &Ctx, sh: &Shape) -> Shape {
    let mut out = sh.clone();
    out.xfrm = Some(resolve::xfrm(ctx, sh));
    if out.fill.is_none()
        && let (Some(f), None) = resolve::fill(ctx, sh)
    {
        out.fill = Some(f);
    }
    if let (Some(t), Some(src)) = (out.text.as_mut(), sh.text.as_ref()) {
        t.body = resolve::body(ctx, sh);
        for (para, from) in t.paragraphs.iter_mut().zip(&src.paragraphs) {
            let lvl = resolve::level_style(ctx, sh, from.level);
            para.props.inherit(&lvl.para);
            for r in &mut para.runs {
                r.props.inherit(&lvl.run);
            }
            para.end_props.inherit(&lvl.run);
        }
    }
    out.ph = None;
    out
}

/// The kinds a box on a master or layout can be made (`target_is_master`: the slide master's
/// five). Slides take only what their layout has; notes and handout headers aren't slide kinds.
pub fn template_kinds(target_is_master: bool) -> &'static [PhType] {
    use PhType::*;
    if target_is_master {
        &[Title, Body, Date, Footer, SlideNum]
    } else {
        &[Title, CtrTitle, SubTitle, Body, Obj, Picture, Chart, Table, Media, Date, Footer, SlideNum]
    }
}

/// Kinds a master or layout has one of (any number of body and content placeholders).
fn single(k: PhType) -> bool {
    k.is_title() || matches!(k, PhType::SubTitle | PhType::Date | PhType::Footer | PhType::SlideNum)
}

/// A box made a placeholder: its own position, still growing with its text if it did.
fn tagged(ctx: &Ctx, sh: &Shape, ph: Placeholder) -> Shape {
    let mut new = if sh.ph.is_some() { bake(ctx, sh) } else { sh.clone() };
    new.xfrm = Some(resolve::xfrm(ctx, sh));
    if sh.text_box {
        let fit = resolve::body(ctx, sh);
        let body = &mut new.text.get_or_insert_with(Default::default).body;
        body.autofit = body.autofit.or(fit.autofit);
        body.wrap = body.wrap.or(fit.wrap);
    }
    new.text_box = false;
    new.ph = Some(ph);
    new
}

fn assign(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "placeholder.assign";
    let kind = str_param(p, "kind").ok_or_else(|| bad(CMD, "give `kind`"))?;
    let st = s.doc()?;
    let doc = &st.doc;
    let target = st.selection.target;
    let shapes = st.shapes();
    let id = match id_param(p, "id", CMD)? {
        Some(id) => id,
        None => *st.selection.shapes.first().ok_or_else(|| bad(CMD, "select a shape"))?,
    };
    let sh = shapes.iter().find(|x| x.id == id).ok_or_else(|| bad(CMD, "not a shape here (shapes in groups can't be placeholders)"))?;
    let ctx = super::text::ctx_for(doc, &st.selection).ok_or_else(|| bad(CMD, "no layout"))?;
    // (shape to replace, its replacement or None to delete)
    let mut changes: Vec<(ShapeId, Option<Shape>)> = vec![];
    let mut untagged = None;
    // An empty placeholder giving up its place goes; one with text stays as a plain box.
    let mut give_up = |old: &Shape, changes: &mut Vec<(ShapeId, Option<Shape>)>| {
        let empty = old.text.as_ref().is_none_or(|t| t.is_empty());
        changes.push((old.id, if empty { None } else { Some(bake(&ctx, old)) }));
        untagged = Some((if empty { "removed" } else { "untagged" }, old.id));
    };
    if kind == "none" {
        if sh.ph.is_none() {
            return Ok(json!({"id": id, "kind": "none"}));
        }
        changes.push((id, Some(bake(&ctx, sh))));
    } else {
        let want = PhType::from_xml(kind);
        if want.xml() != kind {
            return Err(bad(CMD, format!("unknown placeholder kind `{kind}`")));
        }
        if !matches!(sh.kind, ShapeKind::Shape) {
            return Err(bad(CMD, "only shapes with text can be made placeholders here"));
        }
        match target {
            crate::Target::Slides => {
                let layout = ctx.layout.ok_or_else(|| bad(CMD, "this slide has no layout"))?;
                let places: Vec<&Shape> = layout.shapes.iter().filter(|l| l.ph_type().is_some_and(|t| fits(want, t))).collect();
                if places.is_empty() {
                    return Err(bad(CMD, format!("this slide's layout ({}) has no {} placeholder", layout.name, want.xml())));
                }
                // Which other shapes on the slide already hold each place.
                let holder = |place: &Shape| {
                    shapes.iter().find(|o| o.id != id && o.ph.is_some() && resolve::parents(&ctx, o).0.is_some_and(|l| l.id == place.id))
                };
                let place = places.iter().find(|pl| holder(pl).is_none()).or(places.first()).copied().ok_or_else(|| bad(CMD, "no place"))?;
                if let Some(old) = holder(place) {
                    give_up(old, &mut changes);
                }
                let lph = place.ph.clone().unwrap_or_default();
                changes
                    .push((id, Some(tagged(&ctx, sh, Placeholder { kind: lph.kind, idx: lph.idx, vertical: lph.vertical, ..Default::default() }))));
            }
            crate::Target::Master { .. } | crate::Target::Layout { .. } => {
                let master = matches!(target, crate::Target::Master { .. });
                if !template_kinds(master).contains(&want) {
                    let on = if master { "the slide master" } else { "a layout" };
                    return Err(bad(CMD, format!("{on} can't have a {kind} placeholder (headers are for notes and handouts)")));
                }
                // The one of its kind already there gives up its place (and its index).
                let old =
                    (single(want) || master).then(|| shapes.iter().find(|o| o.id != id && o.ph_type().is_some_and(|t| same_kind(t, want)))).flatten();
                let idx = match (want.is_title(), old.and_then(|o| o.ph.as_ref())) {
                    (true, _) => 0,
                    (_, Some(ph)) => ph.idx,
                    _ => shapes.iter().filter_map(|x| x.ph.as_ref().map(|p| p.idx)).max().unwrap_or(0) + 1,
                };
                if let Some(old) = old {
                    give_up(old, &mut changes);
                }
                changes.push((id, Some(tagged(&ctx, sh, Placeholder { kind: want, idx, ..Default::default() }))));
            }
            crate::Target::Handout | crate::Target::NotesMaster => {
                return Err(bad(CMD, "notes and handout placeholders are shown or hidden from their master's Placeholders"));
            }
        }
    }
    let r = s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(CMD, "nothing to change"))?;
        for (cid, new) in changes {
            match new {
                Some(n) => {
                    if let Some(x) = shapes.iter_mut().find(|x| x.id == cid) {
                        *x = n;
                    }
                }
                None => shapes.retain(|x| x.id != cid),
            }
        }
        let left: Vec<ShapeId> = shapes.iter().map(|x| x.id).collect();
        sel.shapes.retain(|x| left.contains(x));
        let mut out = json!({"id": id, "kind": kind});
        if let Some((key, u)) = untagged {
            out[key] = json!(u);
        }
        Ok(out)
    })?;
    // Its text may be set larger now: a box that grows with its text does.
    super::text::fit_text_box(s, id)?;
    Ok(r)
}

#[cfg(test)]
#[path = "placeholder_tests.rs"]
mod tests;
