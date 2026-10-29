//! Selection pane: picking objects from the list, Show All and Hide All. Its other actions are
//! ordinary commands: `shape.visible` (eye), `shape.lock` (padlock), `shape.rename`,
//! `arrange.bringForward` / `arrange.sendBackward` (arrows) and `arrange.reorder` (drag and drop).

use deckcraft_model::{Shape, ShapeId};
use serde_json::{Value, json};

use super::{CommandSpec, bad, bool_or, cmd, has_slide, id_param, ids_param, ok};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "selection.select",
            "Select",
            ["Selection Pane"],
            None,
            "{ids | id, toggle?: bool (Ctrl+click), add?: bool} → {selected}: objects in groups too; a hidden object picked from the list is shown",
            has_slide,
            select
        ),
        cmd!("selection.showAll", "Show All", ["Selection Pane"], None, "{} shows every object on the slide, inside groups too", has_slide, show_all),
        cmd!("selection.hideAll", "Hide All", ["Selection Pane"], None, "{} hides every top-level object on the slide", has_slide, hide_all),
    ]
}

/// The Selection pane's click. Objects inside groups and top-level objects can't be selected
/// together (Ctrl+click on the other kind is ignored, as in PowerPoint), and picking a hidden
/// object from the list shows it again.
fn select(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = match ids_param(p, "ids") {
        Some(ids) => ids,
        None => id_param(p, "id", "selection.select")?.map(|i| vec![i]).unwrap_or_default(),
    };
    let (toggle, add) = (bool_or(p, "toggle", false), bool_or(p, "add", false));
    let st = s.doc()?;
    let in_group = |id: &ShapeId| deckcraft_model::parent_of(st.shapes(), *id).map(|p| p.is_some());
    let mut valid: Vec<ShapeId> = ids.iter().copied().filter(|i| in_group(i).is_some()).collect();
    if valid.is_empty() {
        return Err(bad("selection.select", "no such shape"));
    }
    let kind = valid.first().and_then(in_group);
    valid.retain(|i| in_group(i) == kind);
    let keep: Vec<ShapeId> = if toggle || add {
        let current = st.selection.shapes.clone();
        if current.first().is_some_and(|c| in_group(c) != kind) {
            // Can't mix objects in groups with top-level ones: nothing changes.
            return Ok(json!({"selected": current}));
        }
        current
    } else {
        vec![]
    };
    let mut next = keep;
    for i in &valid {
        match next.iter().position(|x| x == i) {
            Some(k) if toggle => {
                next.remove(k);
            }
            Some(_) => {}
            None => next.push(*i),
        }
    }
    let reveal: Vec<ShapeId> = next.iter().copied().filter(|i| valid.contains(i) && st.shape(*i).is_some_and(|x| x.hidden)).collect();
    if reveal.is_empty() {
        s.select(|_, sel| {
            sel.text = None;
            sel.cells = None;
            sel.shapes = next.clone();
        })?;
    } else {
        s.edit(|doc, sel| {
            let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("selection.select", "no slide"))?;
            for id in &reveal {
                if let Some(sh) = deckcraft_model::find_shape_mut(shapes, *id) {
                    sh.hidden = false;
                }
            }
            sel.text = None;
            sel.cells = None;
            sel.shapes = next.clone();
            Ok(())
        })?;
    }
    Ok(json!({"selected": s.doc()?.selection.shapes}))
}

fn show_all(s: &mut Session, _p: &Value) -> Result<Value> {
    fn any_hidden(v: &[Shape]) -> bool {
        let mut found = false;
        deckcraft_model::walk(v, &mut |sh, _| found |= sh.hidden);
        found
    }
    if !any_hidden(s.doc()?.shapes()) {
        return ok();
    }
    s.edit(|doc, sel| {
        fn rec(v: &mut [Shape], depth: usize) {
            for sh in v {
                sh.hidden = false;
                if depth < 64
                    && let Some(c) = sh.children_mut()
                {
                    rec(c, depth + 1);
                }
            }
        }
        rec(crate::shapes_mut(doc, sel).ok_or_else(|| bad("selection.showAll", "no slide"))?, 0);
        Ok(())
    })?;
    ok()
}

fn hide_all(s: &mut Session, _p: &Value) -> Result<Value> {
    if s.doc()?.shapes().iter().all(|x| x.hidden) {
        return ok();
    }
    s.edit(|doc, sel| {
        for sh in crate::shapes_mut(doc, sel).ok_or_else(|| bad("selection.hideAll", "no slide"))? {
            sh.hidden = true;
        }
        Ok(())
    })?;
    ok()
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
