//! Generated elements: Harvey balls and checkboxes.
//!
//! Each is a group whose `generated` field says what it shows (a ball's value, a checkbox's state).
//! Its children are drawn from that inside the largest centred square of the group's child
//! rectangle, so a stretched group still shows a circle. Every edit of the description redraws
//! them, as SmartArt redraws a diagram from its outline. Resizing the group redraws them too, and
//! the parts can't be moved, resized or deleted on their own ([`keep_whole`]). Hand-formatting a
//! part is allowed; the next redraw replaces it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_geom::Xfrm;
use deckcraft_model::{CheckState, Checkbox, Generated, HarveyBall, Presentation, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::insert::{default_rect, first_free_name};
use super::{CommandSpec, NO_ID_SPACE, bad, cmd, color_value, current_names, current_shapes, has_doc, id_param};
use crate::{EngineError, Result, Session};

mod checkbox;
mod harvey;

pub use harvey::wedge_end;

/// Why `harveyBall.set` is greyed out. `Session::execute` still runs it when `id` names a ball.
pub const SELECT_HARVEY: &str = "select a Harvey ball";
/// Why `checkbox.set` and `checkbox.toggle` are greyed out (see [`SELECT_HARVEY`]).
pub const SELECT_CHECKBOX: &str = "select a checkbox";
const NOT_HARVEY: &str = "not a Harvey ball";
const NOT_CHECKBOX: &str = "not a checkbox";
/// The parts are drawn from the group's description; moved on their own they'd no longer match it.
pub const PARTS_STAY: &str = "the parts of a Harvey ball or checkbox can't be moved, resized or deleted on their own: select the whole element";

/// Default side of an inserted element, in points.
const DEFAULT_SIZE: f64 = 14.0;
/// `size` limits, in points.
const SIZE_RANGE: std::ops::RangeInclusive<f64> = 4.0..=400.0;
/// Largest `rect` side (220 in, the Size and Position dialog's limit) and coordinate, in points.
const MAX_SIDE: f64 = 15_840.0;
const MAX_POS: f64 = 100_000.0;
/// Deepest group nesting walked when resolving a target or checking an edit.
const MAX_DEPTH: usize = 64;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "harveyBall.set",
            "Harvey Ball",
            ["Shape Format", "Harvey Ball"],
            None,
            "{id?, value?: 0..=of, of?: 1..=100, fill?: color, background?: color|null, outline?: color|null, outlineWidth?: pt 0..=20} → {id} (id: the ball or one of its shapes; default every selected ball)",
            harvey_selected,
            set_harvey_ball
        ),
        cmd!(
            "checkbox.set",
            "Checkbox",
            ["Shape Format", "Checkbox"],
            None,
            "{id?, state?: checked|unchecked|crossed, color?: color, boxed?: bool} → {id} (id: the checkbox or one of its shapes; default every selected checkbox)",
            checkbox_selected,
            set_checkbox
        ),
        cmd!(
            "checkbox.toggle",
            "Toggle Checkbox",
            ["Shape Format", "Checkbox"],
            None,
            "{id?} → {id, state} (checked ↔ unchecked, crossed → checked; Space on a selected checkbox does the same)",
            checkbox_selected,
            toggle_checkbox
        ),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Harvey,
    Checkbox,
}

impl Kind {
    fn of(g: &Generated) -> Kind {
        match g {
            Generated::HarveyBall(_) => Kind::Harvey,
            Generated::Checkbox(_) => Kind::Checkbox,
        }
    }
    fn not(self) -> &'static str {
        match self {
            Kind::Harvey => NOT_HARVEY,
            Kind::Checkbox => NOT_CHECKBOX,
        }
    }
}

// ---------- drawing ----------

/// The largest square centred in `a`.
fn square(a: Xfrm) -> Xfrm {
    let s = a.w.min(a.h).max(0.0);
    Xfrm::new(a.x + (a.w - s) / 2.0, a.y + (a.h - s) / 2.0, s, s)
}

/// The shapes `g` draws inside the largest centred square of `area` (child space), with id 0.
pub fn draw(g: &Generated, area: Xfrm) -> std::result::Result<Vec<Shape>, String> {
    if ![area.x, area.y, area.w, area.h].iter().all(|v| v.is_finite()) {
        return Err("the element's box isn't a finite size".into());
    }
    let sq = square(area);
    match g {
        Generated::HarveyBall(b) => harvey::shapes(b, sq),
        Generated::Checkbox(c) => Ok(checkbox::shapes(c, sq)),
    }
}

/// The next free shape id, handed out as [`Presentation::alloc_id`] does while the shape tree is
/// borrowed; the caller writes it back to `next_id`.
struct Ids(u32);

/// Redraw group `sh` from `g`. Its child space is set to its box's size first, so the parts are
/// drawn 1:1 and a stretched group draws a circle, not an ellipse. The old children's ids are
/// reused in order (animations and selections keep pointing at a part); extra parts get new ids.
fn rebuild(sh: &mut Shape, g: Generated, ids: &mut Ids) -> std::result::Result<(), String> {
    let frame = sh.xfrm;
    let ShapeKind::Group { children, child } = &mut sh.kind else { return Err("not a group".into()) };
    let mut space = *child;
    if let Some(x) = frame {
        space.w = x.w;
        space.h = x.h;
    }
    // Drawn and numbered before anything changes, so a failure leaves the group as it was.
    let mut kids = draw(&g, space)?;
    let mut next = ids.0;
    for (k, kid) in kids.iter_mut().enumerate() {
        kid.id = match children.get(k) {
            Some(old) => old.id,
            None => {
                next = next.max(1).checked_add(1).ok_or(NO_ID_SPACE)?;
                ShapeId(next)
            }
        };
    }
    ids.0 = next;
    *child = space;
    *children = kids;
    sh.generated = Some(g);
    Ok(())
}

/// Redraw each group in `plans` with its new description, in one edit.
fn apply(doc: &mut Presentation, sel: &crate::Selection, cmd: &str, plans: Vec<(ShapeId, Generated)>) -> Result<()> {
    let mut ids = Ids(doc.next_id);
    let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
    for (id, g) in plans {
        let sh = deckcraft_model::find_shape_mut(shapes, id).ok_or_else(|| bad(cmd, "no such shape"))?;
        rebuild(sh, g, &mut ids).map_err(|e| bad(cmd, e))?;
    }
    doc.next_id = ids.0;
    Ok(())
}

// ---------- targets ----------

/// The generated group of kind `k` that `id` is or is inside: the nearest such ancestor.
fn owner(shapes: &[Shape], id: ShapeId, k: Kind) -> Option<ShapeId> {
    fn path<'a>(shapes: &'a [Shape], id: ShapeId, out: &mut Vec<&'a Shape>, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        for sh in shapes {
            out.push(sh);
            if sh.id == id || path(sh.children(), id, out, depth + 1) {
                return true;
            }
            out.pop();
        }
        false
    }
    let mut chain = vec![];
    if !path(shapes, id, &mut chain, 0) {
        return None;
    }
    chain.iter().rev().find(|s| s.generated.as_ref().is_some_and(|g| Kind::of(g) == k)).map(|s| s.id)
}

/// The selected elements of kind `k`, each once, in selection order. One walk of the tree however
/// much is selected: the ribbon asks every frame.
fn selected_of(st: &crate::DocState, k: Kind) -> Vec<ShapeId> {
    fn walk(list: &[Shape], k: Kind, owner: Option<ShapeId>, wanted: &HashSet<ShapeId>, found: &mut HashMap<ShapeId, ShapeId>, depth: usize) {
        for sh in list {
            let own = if sh.generated.as_ref().is_some_and(|g| Kind::of(g) == k) { Some(sh.id) } else { owner };
            if let Some(o) = own
                && wanted.contains(&sh.id)
            {
                found.insert(sh.id, o);
            }
            if depth < MAX_DEPTH {
                walk(sh.children(), k, own, wanted, found, depth + 1);
            }
        }
    }
    let Some(shapes) = current_shapes(&st.doc, &st.selection) else { return vec![] };
    if st.selection.shapes.is_empty() {
        return vec![];
    }
    let wanted: HashSet<ShapeId> = st.selection.shapes.iter().copied().collect();
    let mut found = HashMap::new();
    walk(shapes, k, None, &wanted, &mut found, 0);
    let mut seen = HashSet::new();
    st.selection.shapes.iter().filter_map(|id| found.get(id).copied()).filter(|g| seen.insert(*g)).collect()
}

/// The element the selection is (or is inside), for the ribbon: its group id and description.
pub fn selected(st: &crate::DocState) -> Option<(ShapeId, Generated)> {
    [Kind::Harvey, Kind::Checkbox].into_iter().find_map(|k| {
        let id = *selected_of(st, k).first()?;
        Some((id, st.shape(id)?.generated.clone()?))
    })
}

/// Whether the selection holds a checkbox (Space toggles it).
pub(crate) fn checkbox_in_selection(s: &Session) -> bool {
    s.active().is_some_and(|st| !selected_of(st, Kind::Checkbox).is_empty())
}

fn harvey_selected(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|st| !selected_of(st, Kind::Harvey).is_empty()) { Ok(()) } else { Err(SELECT_HARVEY.into()) }
}

fn checkbox_selected(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if checkbox_in_selection(s) { Ok(()) } else { Err(SELECT_CHECKBOX.into()) }
}

/// The elements a `*.set` command edits: the one `id` names (it or one of its parts), else every
/// selected one of kind `k`.
fn targets_of(s: &Session, p: &Value, cmd: &str, k: Kind) -> Result<Vec<ShapeId>> {
    let st = s.doc()?;
    let shapes = current_shapes(&st.doc, &st.selection).ok_or_else(|| bad(cmd, "no slide"))?;
    if let Some(id) = id_param(p, "id", cmd)? {
        return owner(shapes, id, k).map(|g| vec![g]).ok_or_else(|| bad(cmd, k.not()));
    }
    let v = selected_of(st, k);
    if v.is_empty() { Err(bad(cmd, k.not())) } else { Ok(v) }
}

/// Each target's description, changed by `f` (which validates it), ready for [`apply`].
fn plan(
    doc: &Presentation,
    sel: &crate::Selection,
    cmd: &str,
    ids: &[ShapeId],
    mut f: impl FnMut(Generated) -> Result<Generated>,
) -> Result<Vec<(ShapeId, Generated)>> {
    let shapes = current_shapes(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let g = deckcraft_model::find_shape(shapes, *id).and_then(|x| x.generated.clone()).ok_or_else(|| bad(cmd, "no such shape"))?;
        out.push((*id, f(g)?));
    }
    Ok(out)
}

// ---------- params ----------

/// A whole-number param (`3`, or `3.0`): `Ok(None)` when absent, an error when it isn't one.
fn whole_param(p: &Value, key: &str, cmd: &str) -> Result<Option<u16>> {
    let Some(v) = p.get(key) else { return Ok(None) };
    let n =
        v.as_u64().or_else(|| v.as_f64().filter(|f| f.is_finite() && f.fract() == 0.0 && *f >= 0.0 && *f <= f64::from(u16::MAX)).map(|f| f as u64));
    n.and_then(|n| u16::try_from(n).ok()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a whole number, not {v}")))
}

/// A number of points in `range`: `Ok(None)` when absent.
fn points_param(p: &Value, key: &str, cmd: &str, range: std::ops::RangeInclusive<f64>) -> Result<Option<f64>> {
    let Some(v) = p.get(key) else { return Ok(None) };
    match v.as_f64() {
        Some(n) if n.is_finite() && range.contains(&n) => Ok(Some(n)),
        _ => Err(bad(cmd, format!("`{key}` must be a number of points from {} to {}, not {v}", range.start(), range.end()))),
    }
}

/// A colour param: `Ok(None)` when absent, an error when it doesn't parse.
fn colour_param(p: &Value, key: &str, cmd: &str) -> Result<Option<deckcraft_model::ColorRef>> {
    let Some(v) = p.get(key) else { return Ok(None) };
    color_value(v).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` isn't a colour: {v}")))
}

/// A colour param that `null` turns off: `Ok(None)` when absent, `Ok(Some(None))` for `null`.
fn optional_colour_param(p: &Value, key: &str, cmd: &str) -> Result<Option<Option<deckcraft_model::ColorRef>>> {
    match p.get(key) {
        Some(Value::Null) => Ok(Some(None)),
        _ => Ok(colour_param(p, key, cmd)?.map(Some)),
    }
}

/// `rect`: `[x, y, w, h]` in points with a positive width and height. Unlike other insert commands,
/// a negative size is an error rather than turned positive.
fn rect_param(p: &Value, cmd: &str) -> Result<Option<Xfrm>> {
    let Some(v) = p.get("rect") else { return Ok(None) };
    let n: Option<Vec<f64>> = v.as_array().filter(|a| a.len() == 4).and_then(|a| a.iter().map(|x| x.as_f64().filter(|f| f.is_finite())).collect());
    let Some([x, y, w, h]) = n.as_deref().and_then(|n| <[f64; 4]>::try_from(n).ok()) else {
        return Err(bad(cmd, format!("`rect` must be [x, y, w, h] in points, not {v}")));
    };
    if !(w > 0.0 && h > 0.0) {
        return Err(bad(cmd, "`rect` needs a positive width and height"));
    }
    if w > MAX_SIDE || h > MAX_SIDE || x.abs() > MAX_POS || y.abs() > MAX_POS {
        return Err(bad(cmd, "`rect` is too large"));
    }
    Ok(Some(Xfrm::new(x, y, w, h)))
}

/// `b` with the Harvey-ball keys of `p` merged in, checked as a whole.
fn merge_harvey(p: &Value, mut b: HarveyBall, cmd: &str) -> Result<HarveyBall> {
    if let Some(v) = whole_param(p, "value", cmd)? {
        b.value = v;
    }
    if let Some(v) = whole_param(p, "of", cmd)? {
        b.of = v;
    }
    if let Some(c) = colour_param(p, "fill", cmd)? {
        b.fill = c;
    }
    if let Some(c) = optional_colour_param(p, "background", cmd)? {
        b.background = c;
    }
    if let Some(c) = optional_colour_param(p, "outline", cmd)? {
        b.outline = c;
    }
    if let Some(w) = points_param(p, "outlineWidth", cmd, 0.0..=deckcraft_model::generated::MAX_OUTLINE_WIDTH)? {
        b.outline_width = w;
    }
    b.check().map_err(|e| bad(cmd, e))?;
    Ok(b)
}

/// `c` with the checkbox keys of `p` merged in.
fn merge_checkbox(p: &Value, mut c: Checkbox, cmd: &str) -> Result<Checkbox> {
    if let Some(v) = p.get("state") {
        c.state = v.as_str().and_then(CheckState::from_name).ok_or_else(|| bad(cmd, format!("unknown state {v}: checked, unchecked or crossed")))?;
    }
    if let Some(col) = colour_param(p, "color", cmd)? {
        c.color = col;
    }
    if let Some(v) = p.get("boxed") {
        c.boxed = v.as_bool().ok_or_else(|| bad(cmd, format!("`boxed` must be true or false, not {v}")))?;
    }
    Ok(c)
}

// ---------- commands ----------

/// The new element's frame: `rect`, else a square of `size` points where new shapes go.
fn frame(s: &Session, p: &Value, cmd: &str) -> Result<Xfrm> {
    let size = points_param(p, "size", cmd, SIZE_RANGE)?.unwrap_or(DEFAULT_SIZE);
    Ok(match rect_param(p, cmd)? {
        Some(r) => r,
        None => default_rect(s, size, size),
    })
}

fn insert(s: &mut Session, cmd: &str, g: Generated, rect: Xfrm, base: &str) -> Result<Value> {
    let id = s.edit(|doc, sel| {
        let mut kids = draw(&g, rect).map_err(|e| bad(cmd, e))?;
        for k in &mut kids {
            k.id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
        }
        let id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
        let (taken, count) = current_names(doc, sel);
        let group = Shape {
            id,
            name: first_free_name(base, &taken, count),
            xfrm: Some(rect),
            kind: ShapeKind::Group { children: kids, child: rect },
            generated: Some(g),
            ..Default::default()
        };
        crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?.push(group);
        sel.shapes = vec![id];
        sel.text = None;
        Ok(id)
    })?;
    Ok(json!({"id": id}))
}

pub(super) fn insert_harvey_ball(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "insert.harveyBall";
    let b = merge_harvey(p, HarveyBall::default(), CMD)?;
    let rect = frame(s, p, CMD)?;
    insert(s, CMD, Generated::HarveyBall(b), rect, "Harvey Ball")
}

pub(super) fn insert_checkbox(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "insert.checkbox";
    let c = merge_checkbox(p, Checkbox::default(), CMD)?;
    let rect = frame(s, p, CMD)?;
    insert(s, CMD, Generated::Checkbox(c), rect, "Checkbox")
}

fn answer(ids: &[ShapeId]) -> Value {
    match ids {
        [one] => json!({"id": one}),
        _ => json!({"id": ids.first(), "ids": ids}),
    }
}

fn set_harvey_ball(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "harveyBall.set";
    let ids = targets_of(s, p, CMD, Kind::Harvey)?;
    s.edit(|doc, sel| {
        let plans = plan(doc, sel, CMD, &ids, |g| match g {
            Generated::HarveyBall(b) => Ok(Generated::HarveyBall(merge_harvey(p, b, CMD)?)),
            _ => Err(bad(CMD, NOT_HARVEY)),
        })?;
        apply(doc, sel, CMD, plans)
    })?;
    Ok(answer(&ids))
}

fn set_checkbox(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "checkbox.set";
    let ids = targets_of(s, p, CMD, Kind::Checkbox)?;
    s.edit(|doc, sel| {
        let plans = plan(doc, sel, CMD, &ids, |g| match g {
            Generated::Checkbox(c) => Ok(Generated::Checkbox(merge_checkbox(p, c, CMD)?)),
            _ => Err(bad(CMD, NOT_CHECKBOX)),
        })?;
        apply(doc, sel, CMD, plans)
    })?;
    Ok(answer(&ids))
}

fn toggle_checkbox(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "checkbox.toggle";
    let ids = targets_of(s, p, CMD, Kind::Checkbox)?;
    let first = s.edit(|doc, sel| {
        let mut first = None;
        let plans = plan(doc, sel, CMD, &ids, |g| match g {
            Generated::Checkbox(mut c) => {
                c.state = match c.state {
                    CheckState::Checked => CheckState::Unchecked,
                    CheckState::Unchecked | CheckState::Crossed => CheckState::Checked,
                };
                first.get_or_insert(c.state);
                Ok(Generated::Checkbox(c))
            }
            _ => Err(bad(CMD, NOT_CHECKBOX)),
        })?;
        apply(doc, sel, CMD, plans)?;
        Ok(first)
    })?;
    let mut v = answer(&ids);
    if let (Some(o), Some(state)) = (v.as_object_mut(), first) {
        o.insert("state".into(), json!(state.name()));
    }
    Ok(v)
}

// ---------- Shape Fill and Shape Outline ----------

/// What [`route_format`] left for `shape.fill` or `shape.line` to do.
pub(crate) enum Format {
    /// No target is a Harvey ball or checkbox: format as asked.
    Unchanged,
    /// Every target was one, and is done: the command's answer.
    Done(Value),
    /// Some were: format the others, named in these params' `ids`.
    Rest(Value),
}

/// Shape Fill and Shape Outline on a Harvey ball or checkbox (the element itself, not one of its
/// parts) change its description through `harveyBall.set` or `checkbox.set`, so a redraw keeps
/// the colour: fill → `fill`, outline colour or none → `outline`, weight → `outlineWidth`; on a
/// checkbox either colour → `color`; `reset` → the defaults. What a description can't hold (a
/// gradient, dashes, a checkbox weight…) is refused. A part named by id is formatted by hand as
/// any shape is, until the next redraw.
pub(crate) fn route_format(s: &mut Session, p: &Value, cmd: &str) -> Result<Format> {
    let ids = super::targets(s, p)?;
    let (mut elements, mut others) = (vec![], vec![]);
    {
        let st = s.doc()?;
        for id in ids {
            match st.shape(id).and_then(|sh| sh.generated.clone()) {
                Some(g) => elements.push((id, g)),
                None => others.push(id.0),
            }
        }
    }
    if elements.is_empty() {
        return Ok(Format::Unchanged);
    }
    let n = elements.len();
    for (id, g) in elements {
        let (set, mut q) = element_params(p, cmd, &g)?;
        if let Some(o) = q.as_object_mut() {
            o.insert("id".into(), json!(id));
        }
        s.execute(set, &q)?;
    }
    Ok(if others.is_empty() { Format::Done(Value::from(n)) } else { Format::Rest(super::with_param(p, "ids", json!(others))) })
}

/// The `harveyBall.set` or `checkbox.set` params a `shape.fill` / `shape.line` call `p` makes for
/// element `g`.
fn element_params(p: &Value, cmd: &str, g: &Generated) -> Result<(&'static str, Value)> {
    let given = |k: &str| p.get(k).is_some_and(|v| !v.is_null() && *v != Value::Bool(false));
    let what = match g {
        Generated::HarveyBall(_) => "a Harvey ball",
        Generated::Checkbox(_) => "a checkbox",
    };
    let refuse = |keys: &[&str], takes: &str| match keys.iter().find(|k| given(k)) {
        Some(k) => Err(bad(cmd, format!("{what} takes {takes}, not `{k}`"))),
        None => Ok(()),
    };
    let colour = || -> Result<Option<Value>> {
        match p.get("color").filter(|v| !v.is_null()) {
            Some(v) if color_value(v).is_some() => Ok(Some(v.clone())),
            Some(v) => Err(bad(cmd, format!("`color` isn't a colour: {v}"))),
            None => Ok(None),
        }
    };
    let reset = given("reset");
    let tx1 = || json!("tx1");
    match (cmd, g) {
        ("shape.fill", _) => {
            refuse(&["none", "background", "gradient", "picture", "pattern", "transparency", "pictureProps"], "one fill colour")?;
            let c = if reset { tx1() } else { colour()?.ok_or_else(|| bad(cmd, format!("give {what} a `color`")))? };
            Ok(match g {
                Generated::HarveyBall(_) => ("harveyBall.set", json!({"fill": c})),
                Generated::Checkbox(_) => ("checkbox.set", json!({"color": c})),
            })
        }
        ("shape.line", Generated::HarveyBall(b)) => {
            refuse(&["dash", "cap", "join", "compound", "head", "tail", "headSize", "tailSize"], "an outline colour and weight")?;
            if reset {
                return Ok(("harveyBall.set", json!({"outline": tx1(), "outlineWidth": HarveyBall::default().outline_width})));
            }
            let mut q = serde_json::Map::new();
            if given("none") {
                q.insert("outline".into(), Value::Null);
            }
            if let Some(c) = colour()? {
                q.insert("outline".into(), c);
            }
            if let Some(w) = p.get("width").filter(|v| !v.is_null()) {
                let w = w.as_f64().filter(|w| w.is_finite() && (0.0..=deckcraft_model::generated::MAX_OUTLINE_WIDTH).contains(w));
                let w = w.ok_or_else(|| bad(cmd, format!("{what}'s outline weight is 0 to {} pt", deckcraft_model::generated::MAX_OUTLINE_WIDTH)))?;
                q.insert("outlineWidth".into(), json!(w));
                // A weight on a ball without a ring gives it one, as Shape Outline does on a shape.
                if b.outline.is_none() && !q.contains_key("outline") {
                    q.insert("outline".into(), tx1());
                }
            }
            if q.is_empty() {
                return Err(bad(cmd, format!("give {what} an outline `color`, `none` or `width`")));
            }
            Ok(("harveyBall.set", Value::Object(q)))
        }
        ("shape.line", Generated::Checkbox(_)) => {
            refuse(&["none", "width", "dash", "cap", "join", "compound", "head", "tail", "headSize", "tailSize"], "one colour")?;
            let c = if reset { tx1() } else { colour()?.ok_or_else(|| bad(cmd, format!("give {what} a `color`")))? };
            Ok(("checkbox.set", json!({"color": c})))
        }
        _ => Err(bad(cmd, format!("{what} is formatted with its own command"))),
    }
}

// ---------- after every edit ----------

/// After an edit (`Session::edit`): keep each generated group whole. One whose box was resized
/// (its box no longer matches its child space) is redrawn inside it, so a ball stays round. One
/// whose parts were moved, resized, reordered or deleted while its description and box stayed
/// put refuses the edit ([`PARTS_STAY`]): the parts are drawn from the description. A group a
/// command redrew (its description changed) is left as that command made it. Only slides, masters
/// and layouts the edit touched are looked at.
pub(crate) fn keep_whole(old: &Presentation, new: &mut Presentation) -> Result<()> {
    let mut ids = Ids(new.next_id);
    let old_slides: HashMap<_, _> = old.slides.iter().map(|s| (s.id, s)).collect();
    for slide in new.slides.iter_mut() {
        let Some(before) = old_slides.get(&slide.id) else { continue };
        if Arc::ptr_eq(before, slide) || !holds_generated(&slide.shapes) {
            continue;
        }
        keep_list(&before.shapes, &mut Arc::make_mut(slide).shapes, &mut ids)?;
    }
    for master in new.masters.iter_mut() {
        if let Some(before) = old.masters.iter().find(|m| m.id == master.id) {
            keep_master(before, master, &mut ids)?;
        }
    }
    for (before, after) in [(old.notes_master.as_ref(), new.notes_master.as_mut()), (old.handout_master.as_ref(), new.handout_master.as_mut())] {
        if let (Some(before), Some(after)) = (before, after) {
            keep_master(before, after, &mut ids)?;
        }
    }
    new.next_id = ids.0;
    Ok(())
}

fn keep_master(before: &Arc<deckcraft_model::Master>, after: &mut Arc<deckcraft_model::Master>, ids: &mut Ids) -> Result<()> {
    if Arc::ptr_eq(before, after) || !(holds_generated(&after.shapes) || after.layouts.iter().any(|l| holds_generated(&l.shapes))) {
        return Ok(());
    }
    let m = Arc::make_mut(after);
    keep_list(&before.shapes, &mut m.shapes, ids)?;
    for layout in m.layouts.iter_mut() {
        if let Some(b) = before.layouts.iter().find(|l| l.id == layout.id) {
            keep_list(&b.shapes, &mut layout.shapes, ids)?;
        }
    }
    Ok(())
}

fn holds_generated(shapes: &[Shape]) -> bool {
    let mut any = false;
    deckcraft_model::walk(shapes, &mut |s, _| any |= s.generated.is_some());
    any
}

fn keep_list(old: &[Shape], new: &mut [Shape], ids: &mut Ids) -> Result<()> {
    // Found by id anywhere in the old tree: ungrouping an outer group moves an element up a level.
    let mut before: HashMap<ShapeId, &Shape> = HashMap::new();
    deckcraft_model::walk(old, &mut |s, _| {
        if s.generated.is_some() {
            before.insert(s.id, s);
        }
    });
    if before.is_empty() {
        return Ok(());
    }
    visit(new, &before, ids, 0)
}

fn visit(list: &mut [Shape], before: &HashMap<ShapeId, &Shape>, ids: &mut Ids, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for sh in list.iter_mut() {
        if sh.generated.is_some()
            && let Some(old) = before.get(&sh.id)
        {
            keep_one(sh, old, ids)?;
        } else if let ShapeKind::Group { children, .. } = &mut sh.kind {
            visit(children, before, ids, depth + 1)?;
        }
    }
    Ok(())
}

fn keep_one(sh: &mut Shape, old: &Shape, ids: &mut Ids) -> Result<()> {
    let Some(g) = sh.generated.clone() else { return Ok(()) };
    if old.generated.as_ref() != Some(&g) {
        return Ok(());
    }
    // Not mirrored: `shape.flip` refuses it, and a resize handle dragged past the opposite edge
    // keeps the element's orientation (a mirrored ball would fill counter-clockwise).
    if let (Some(x), Some(o)) = (sh.xfrm.as_mut(), old.xfrm) {
        x.flip_h = o.flip_h;
        x.flip_v = o.flip_v;
    }
    let (ShapeKind::Group { children, child }, ShapeKind::Group { children: was, child: was_child }) = (&sh.kind, &old.kind) else { return Ok(()) };
    let touched = sh.xfrm != old.xfrm || child != was_child;
    let stretched = sh.xfrm.is_some_and(|x| (x.w - child.w).abs() > 1e-6 || (x.h - child.h).abs() > 1e-6);
    if touched && stretched {
        // A description a hostile file brought that can't be drawn stays as it was drawn.
        let _ = rebuild(sh, g, ids);
        return Ok(());
    }
    let same = children.len() == was.len() && children.iter().zip(was).all(|(a, b)| a.id == b.id && a.xfrm == b.xfrm);
    if same { Ok(()) } else { Err(EngineError::Other(PARTS_STAY.into())) }
}

#[cfg(test)]
mod tests;
