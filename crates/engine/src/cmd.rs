//! The command registry. Ids follow PowerPoint's ribbon and menu structure.

pub mod animation;
pub mod arrange;
pub mod artistic;
pub mod background;
pub mod compress;
pub mod design;
pub mod edit;
pub mod equation;
pub mod file;
pub mod find;
pub mod format;
pub mod ideas;
pub mod insert;
pub mod inspect;
pub mod media;
pub mod merge;
pub mod picture_style;
pub mod points;
pub mod recolor;
pub mod reuse;
pub mod review;
mod richclip;
pub mod selection;
pub mod shape;
pub mod slide;
pub mod smartart;
pub mod table;
pub mod text;
pub mod three_d;
pub mod transition;
pub mod zoom;

use deckcraft_geom::Xfrm;
use deckcraft_model::ShapeId;
use serde::Serialize;
use serde_json::Value;

use crate::{EngineError, Result, Session};

pub type Run = fn(&mut Session, &Value) -> Result<Value>;
pub type Enabled = fn(&Session) -> std::result::Result<(), String>;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Where it lives: `["Home", "Font"]` (ribbon tab and group) or `["Arrange"]` (menu).
    pub menu: &'static [&'static str],
    pub shortcut: Option<&'static str>,
    /// The parameters as `describe` and `commands` show them, read by [`parse_params`]: `?` marks
    /// an optional key, a spaced ` | ` separates alternative sets of keys (so alternative values
    /// are written unspaced, `ms|null`), `(…)` is a remark and `→` starts the answer.
    pub params: &'static str,
    pub enabled: Enabled,
    pub run: Run,
    pub journal: bool,
    pub undoable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub menu: Vec<&'static str>,
    pub shortcut: Option<&'static str>,
    pub params: &'static str,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<String>,
}

impl CommandSpec {
    /// Holds because `Session::execute` skips the selection check when `id` or `ids` is given.
    pub fn acts_on(&self) -> Option<String> {
        let keys = param_keys(self.params).unwrap_or_default();
        let named = if keys.contains(&"ids") {
            Some("the shapes in `ids`")
        } else if keys.contains(&"id") {
            Some("the shape in `id`")
        } else {
            None
        };
        let reason = (self.enabled)(&Session::with_new()).err()?;
        Some(match (reason.as_str(), named) {
            (SELECT_TEXT_OR_SHAPE, _) => "Acts on the selected text or shapes.".into(),
            (NOTHING_SELECTED, Some(n)) => format!("Acts on {n}, else on the selected shapes."),
            (NOTHING_SELECTED, None) => "Acts on the selected shapes.".into(),
            (SELECT_TWO, Some(n)) => format!("Acts on two or more shapes: {n}, else the selected ones."),
            (SELECT_TWO, None) => "Acts on two or more selected shapes.".into(),
            (NO_TEXT_POINT, _) => "Acts on the text being edited; text.edit starts editing a shape's text.".into(),
            _ => return None,
        })
    }

    pub fn info(&self, s: &Session) -> CommandInfo {
        let e = (self.enabled)(s);
        CommandInfo {
            id: self.id,
            label: self.label,
            menu: self.menu.to_vec(),
            shortcut: self.shortcut,
            params: self.params,
            enabled: e.is_ok(),
            disabled_reason: e.err(),
        }
    }
}

pub fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}
pub fn has_doc(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no presentation open".into())
}
pub fn has_slide(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| !d.doc.slides.is_empty() || d.selection.target != crate::Target::Slides) { Ok(()) } else { Err("no slides".into()) }
}
pub const NOTHING_SELECTED: &str = "nothing selected";
pub const SELECT_TWO: &str = "select two or more objects";
pub const NO_TEXT_POINT: &str = "no text insertion point";
pub const SELECT_TEXT_OR_SHAPE: &str = "select text or a shape";
pub fn has_selection(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| !d.selection.shapes.is_empty()) { Ok(()) } else { Err(NOTHING_SELECTED.into()) }
}
pub fn has_two(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.shapes.len() >= 2) { Ok(()) } else { Err(SELECT_TWO.into()) }
}
pub fn has_text(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some()) { Ok(()) } else { Err(NO_TEXT_POINT.into()) }
}
/// Text editing or selected shapes (formatting commands apply to either).
pub fn has_text_or_shapes(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some() || !d.selection.shapes.is_empty()) { Ok(()) } else { Err(SELECT_TEXT_OR_SHAPE.into()) }
}

pub fn can_undo(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| !d.history.undo.is_empty()) { Ok(()) } else { Err("nothing to undo".into()) }
}
pub fn can_redo(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_some_and(|d| !d.history.redo.is_empty()) { Ok(()) } else { Err("nothing to redo".into()) }
}

macro_rules! cmd {
    ($id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: true, undoable: true }
    };
    (query $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: false, undoable: false }
    };
    (noundo $id:literal, $label:literal, [$($m:literal),*], $sc:expr, $params:literal, $en:expr, $run:expr) => {
        $crate::cmd::CommandSpec { id: $id, label: $label, menu: &[$($m),*], shortcut: $sc, params: $params, enabled: $en, run: $run, journal: true, undoable: false }
    };
}
pub(crate) use cmd;

pub fn command_specs() -> &'static [CommandSpec] {
    static SPECS: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    SPECS.get_or_init(|| {
        let mut v = Vec::new();
        v.extend(file::specs());
        v.extend(edit::specs());
        v.extend(slide::specs());
        v.extend(reuse::specs());
        v.extend(insert::specs());
        v.extend(smartart::specs());
        v.extend(equation::specs());
        v.extend(shape::specs());
        v.extend(background::specs());
        v.extend(picture_style::specs());
        v.extend(recolor::specs());
        v.extend(arrange::specs());
        v.extend(selection::specs());
        v.extend(merge::specs());
        v.extend(points::specs());
        v.extend(text::specs());
        v.extend(format::specs());
        v.extend(design::specs());
        v.extend(ideas::specs());
        v.extend(transition::specs());
        v.extend(animation::specs());
        v.extend(table::specs());
        v.extend(find::specs());
        v.extend(review::specs());
        v.extend(inspect::specs());
        v.extend(media::specs());
        v.extend(artistic::specs());
        v.extend(zoom::specs());
        v.extend(compress::specs());
        v
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

pub(crate) fn param_keys(doc: &str) -> std::result::Result<Vec<&str>, String> {
    let mut keys = vec![];
    for p in parse_params(doc)? {
        if !keys.contains(&p.name) {
            keys.push(p.name);
        }
    }
    Ok(keys)
}

#[derive(Debug, PartialEq)]
pub struct Param<'a> {
    pub name: &'a str,
    pub optional: bool,
    /// What the doc says of it: its values, unit or remark (`rect|roundRect|…`, `[x,y,w,h] pt`).
    pub about: &'a str,
    /// An alternative to the parameter before it in the same object (`color? | none?`).
    pub or: bool,
    /// Which of the doc's alternative objects it belongs to: `{index} | {id}` puts id in object 1.
    pub object: usize,
}

/// The top-level parameters of a params doc: `{preset, rect?: [x,y,w,h] pt} | {id}` gives preset,
/// rect (optional, `[x,y,w,h] pt`) and id (in the second object). Text after `→` is the result
/// and is skipped. Errs where the doc strays from that grammar.
pub fn parse_params(doc: &str) -> std::result::Result<Vec<Param<'_>>, String> {
    let doc = doc.split('→').next().unwrap_or_default();
    let mut params: Vec<Param> = vec![];
    let mut open: Vec<char> = vec![];
    let mut expect_key = false;
    let mut or = false;
    let mut objects: usize = 0;
    let mut about_from: Option<usize> = None;
    let mut chars = doc.char_indices().peekable();
    let mut prev = ' ';
    fn end_about<'a>(doc: &'a str, params: &mut [Param<'a>], from: &mut Option<usize>, to: usize) {
        if let (Some(f), Some(p)) = (from.take(), params.last_mut()) {
            p.about = doc.get(f..to).unwrap_or_default().trim();
        }
    }
    while let Some((i, c)) = chars.next() {
        let in_object = open == ['{'];
        match c {
            '{' | '[' | '(' => {
                open.push(c);
                if open == ['{'] {
                    expect_key = true;
                    objects += 1;
                    or = false;
                }
            }
            '}' | ']' | ')' => {
                if in_object {
                    end_about(doc, &mut params, &mut about_from, i);
                }
                open.pop();
            }
            ',' if in_object => {
                end_about(doc, &mut params, &mut about_from, i);
                expect_key = true;
            }
            '|' if in_object && prev == ' ' && chars.peek().is_some_and(|(_, n)| *n == ' ') => {
                end_about(doc, &mut params, &mut about_from, i);
                expect_key = true;
                or = true;
            }
            c if in_object && expect_key && (c.is_ascii_alphabetic() || c == '_') => {
                let mut end = i + c.len_utf8();
                while let Some((j, n)) = chars.peek().copied().filter(|(_, n)| n.is_ascii_alphanumeric() || *n == '_') {
                    end = j + n.len_utf8();
                    chars.next();
                }
                let after = doc.get(end..).unwrap_or_default();
                let rest = after.trim_start_matches('?').trim_start();
                let at = doc.len() - rest.len();
                about_from = match rest.chars().next() {
                    Some(':') => Some(at + 1),
                    Some('(') => Some(at),
                    Some(',' | '}' | '|') => None,
                    _ => return Err(format!("`{}` is followed by `{}`", &doc[i..end], rest.chars().take(12).collect::<String>())),
                };
                params.push(Param { name: &doc[i..end], optional: after.starts_with('?'), about: "", or, object: objects.saturating_sub(1) });
                or = false;
                expect_key = false;
            }
            c if in_object && expect_key && !c.is_whitespace() => return Err(format!("expected a key at `{}`", &doc[i..])),
            _ => {}
        }
        prev = c;
    }
    Ok(params)
}

// ---------- param helpers ----------

/// The deck has used every object id (a hostile file can arrive that way): it cannot grow, and
/// growing would duplicate ids (#407).
pub(crate) const NO_ID_SPACE: &str = "the presentation has used every object id and can hold no more";

pub(crate) fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}
pub(crate) fn f64_param(p: &Value, key: &str) -> Option<f64> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite())
}
pub(crate) fn f64_or(p: &Value, key: &str, default: f64) -> f64 {
    f64_param(p, key).unwrap_or(default)
}
pub(crate) fn bool_param(p: &Value, key: &str) -> Option<bool> {
    p.get(key).and_then(Value::as_bool)
}
pub(crate) fn bool_or(p: &Value, key: &str, default: bool) -> bool {
    bool_param(p, key).unwrap_or(default)
}
pub(crate) fn str_param<'a>(p: &'a Value, key: &str) -> Option<&'a str> {
    p.get(key).and_then(Value::as_str)
}
pub(crate) fn usize_param(p: &Value, key: &str) -> Option<usize> {
    p.get(key).and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok())
}
/// An index param: `Ok(None)` when absent (callers may fall back to the selection), `Err` when
/// present but not a valid index (negative, non-integer), so a bad index never acts on the selection.
pub(crate) fn opt_usize_param(p: &Value, key: &str, cmd: &str) -> Result<Option<usize>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().and_then(|v| usize::try_from(v).ok()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` is not a valid index: {v}"))),
    }
}
/// A shape id param: `Ok(None)` when absent (callers may fall back to the selection), `Err` when
/// present but not a valid id (negative, non-integer, beyond `u32`), so a bad id never edits the selection.
pub(crate) fn id_param(p: &Value, key: &str, cmd: &str) -> Result<Option<ShapeId>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .map(|v| Some(ShapeId(v)))
            .ok_or_else(|| bad(cmd, format!("`{key}` is not a valid shape id: {v}"))),
    }
}
pub(crate) fn ids_param(p: &Value, key: &str) -> Option<Vec<ShapeId>> {
    p.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).filter_map(|v| u32::try_from(v).ok()).map(ShapeId).collect())
}
/// `[x, y, w, h]` in points.
pub(crate) fn rect_param(p: &Value, key: &str) -> Option<Xfrm> {
    let a = p.get(key)?.as_array()?;
    let f = |i: usize| a.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
    Some(Xfrm::new(f(0)?, f(1)?, f(2)?.abs(), f(3)?.abs()))
}
pub(crate) fn with_param(p: &Value, key: &str, v: Value) -> Value {
    let mut m = p.as_object().cloned().unwrap_or_default();
    m.insert(key.to_string(), v);
    Value::Object(m)
}

/// Targets: `ids` / `id` params, else the selection.
pub(crate) fn targets(s: &Session, p: &Value) -> Result<Vec<ShapeId>> {
    if let Some(a) = p.get("ids").and_then(Value::as_array)
        && let Some(v) = a.iter().find(|v| v.as_u64().and_then(|v| u32::try_from(v).ok()).is_none())
    {
        return Err(bad("shape", format!("`ids` has an invalid shape id: {v}")));
    }
    if let Some(ids) = ids_param(p, "ids")
        && !ids.is_empty()
    {
        return Ok(ids);
    }
    if let Some(id) = id_param(p, "id", "shape")? {
        return Ok(vec![id]);
    }
    Ok(s.doc()?.selection.shapes.clone())
}

pub(crate) fn ok() -> Result<Value> {
    Ok(Value::Null)
}

/// Parse a colour param: `"#RRGGBB"`, `"accent1"`…, or `{"scheme":"accent2","lumMod":75000}`.
/// A preset or system colour keeps its base: `{"preset":"red"}`,
/// `{"system":"windowText","last":"#000000"}`.
pub(crate) fn color_param(p: &Value, key: &str) -> Option<deckcraft_model::ColorRef> {
    let v = p.get(key)?;
    color_value(v)
}

pub(crate) fn color_value(v: &Value) -> Option<deckcraft_model::ColorRef> {
    use deckcraft_model::ColorRef;
    match v {
        Value::String(s) => {
            if let Some(slot) = deckcraft_color::SchemeSlot::from_xml(s) {
                return Some(ColorRef::scheme(slot));
            }
            if let Some(c) = deckcraft_color::Rgba::from_hex(s) {
                return Some(ColorRef::rgb(c));
            }
            deckcraft_color::preset(s).map(ColorRef::rgb)
        }
        Value::Object(o) => {
            let mut c = if let Some(s) = o.get("scheme").and_then(Value::as_str) {
                ColorRef::scheme(deckcraft_color::SchemeSlot::from_xml(s)?)
            } else if let Some(name) = o.get("preset").and_then(Value::as_str) {
                if !deckcraft_color::is_preset_name(name) {
                    return None;
                }
                ColorRef { base: deckcraft_model::ColorBase::Preset { name: name.to_string() }, mods: vec![] }
            } else if let Some(name) = o.get("system").and_then(Value::as_str) {
                if !deckcraft_color::is_system_name(name) {
                    return None;
                }
                let last = deckcraft_color::Rgba::from_hex(o.get("last").and_then(Value::as_str)?)?;
                ColorRef { base: deckcraft_model::ColorBase::System { name: name.to_string(), last }, mods: vec![] }
            } else {
                ColorRef::rgb(deckcraft_color::Rgba::from_hex(o.get("rgb").and_then(Value::as_str)?)?)
            };
            for (k, val) in o {
                if let Some(t) = deckcraft_color::ColorTransform::from_xml(k, val.as_i64().and_then(|x| i32::try_from(x).ok())) {
                    c.mods.push(t);
                }
            }
            Some(c)
        }
        _ => None,
    }
}

/// Look a shape up by id in the edited tree.
pub(crate) fn shape_of(s: &Session, id: ShapeId) -> Result<deckcraft_model::Shape> {
    s.doc()?.shape(id).cloned().ok_or_else(|| EngineError::Other(format!("no shape {id}")))
}

/// Effective box of a shape (placeholders inherit theirs). A table's box is at least as tall as its
/// rows are drawn (rows grow to fit their text), so handles and hit tests match what is shown.
pub fn xfrm_of(doc: &deckcraft_model::Presentation, sel: &crate::Selection, shape: &deckcraft_model::Shape) -> Xfrm {
    let mut x = shape.xfrm.unwrap_or_else(|| ctx_xfrm(doc, sel, shape));
    if let deckcraft_model::ShapeKind::Table(t) = &shape.kind {
        let drawn: f64 = table_drawn_heights(doc, sel, t).iter().sum();
        if drawn.is_finite() && drawn > x.h {
            x.h = drawn;
        }
    }
    x
}

/// Row heights of table `t` as drawn: a stored height is a minimum, rows grow to fit their text.
pub fn table_drawn_heights(doc: &deckcraft_model::Presentation, sel: &crate::Selection, t: &deckcraft_model::Table) -> Vec<f64> {
    match text::ctx_for(doc, sel) {
        Some(ctx) => deckcraft_render::table_row_heights(&ctx, t, &deckcraft_text::NoFields),
        None => t.rows.iter().map(|r| r.height).collect(),
    }
}

fn ctx_xfrm(doc: &deckcraft_model::Presentation, sel: &crate::Selection, shape: &deckcraft_model::Shape) -> Xfrm {
    use deckcraft_model::resolve::{self, Ctx};
    match sel.target {
        crate::Target::Slides => doc.slides.get(sel.slide).and_then(|s| Ctx::for_slide(doc, s)).map(|c| resolve::xfrm(&c, shape)),
        crate::Target::Master { master } => doc.masters.get(master).map(|m| resolve::xfrm(&Ctx::for_master(doc, m), shape)),
        t @ (crate::Target::Handout | crate::Target::NotesMaster) => {
            crate::page_master(doc, t).map(|m| resolve::xfrm(&Ctx::for_master(doc, m), shape))
        }
        crate::Target::Layout { master, layout } => {
            doc.masters.get(master).and_then(|m| m.layouts.get(layout).map(|l| resolve::xfrm(&Ctx::for_layout(doc, m, l), shape)))
        }
    }
    .unwrap_or_default()
}

/// Run `f` on each target shape (copy-on-write), materialising inherited boxes first.
pub(crate) fn edit_shapes(s: &mut Session, p: &Value, cmd: &str, f: impl Fn(&mut deckcraft_model::Shape) -> Result<()>) -> Result<Value> {
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(cmd, "no shapes selected"));
    }
    s.edit(|doc, sel| {
        let boxes: Vec<(ShapeId, Xfrm)> = {
            let st_shapes: Vec<deckcraft_model::Shape> =
                ids.iter().filter_map(|id| current_shapes(doc, sel).and_then(|v| deckcraft_model::find_shape(v, *id)).cloned()).collect();
            st_shapes.iter().map(|sh| (sh.id, xfrm_of(doc, sel, sh))).collect()
        };
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad(cmd, "no slide"))?;
        let mut n = 0;
        for id in &ids {
            if let Some(sh) = deckcraft_model::find_shape_mut(shapes, *id) {
                if sh.xfrm.is_none() {
                    sh.xfrm = boxes.iter().find(|(i, _)| i == id).map(|(_, x)| *x);
                }
                f(sh)?;
                n += 1;
            }
        }
        if n == 0 {
            return Err(bad(cmd, "no such shape"));
        }
        Ok(Value::from(n))
    })
}

/// [`edit_shapes`] for edits a locked object refuses (move, size, rotate, flip, reshape, crop):
/// locked targets are left out, and when every target is locked nothing changes.
pub(crate) fn edit_geometry(s: &mut Session, p: &Value, cmd: &str, f: impl Fn(&mut deckcraft_model::Shape) -> Result<()>) -> Result<Value> {
    let ids = targets(s, p)?;
    let st = s.doc()?;
    let free: Vec<u32> = ids.iter().filter(|id| !st.shape(**id).is_some_and(|x| x.locked)).map(|id| id.0).collect();
    if !ids.is_empty() && free.is_empty() {
        return Err(bad(cmd, "the object is locked"));
    }
    edit_shapes(s, &with_param(p, "ids", Value::from(free)), cmd, f)
}

pub(crate) fn current_shapes<'a>(doc: &'a deckcraft_model::Presentation, sel: &crate::Selection) -> Option<&'a Vec<deckcraft_model::Shape>> {
    match sel.target {
        crate::Target::Slides => doc.slides.get(sel.slide).map(|s| &s.shapes),
        crate::Target::Master { master } => doc.masters.get(master).map(|m| &m.shapes),
        t @ (crate::Target::Handout | crate::Target::NotesMaster) => crate::page_master(doc, t).map(|m| &m.shapes),
        crate::Target::Layout { master, layout } => doc.masters.get(master).and_then(|m| m.layouts.get(layout)).map(|l| &l.shapes),
    }
}

pub(crate) fn current_names(doc: &deckcraft_model::Presentation, sel: &crate::Selection) -> std::collections::HashSet<String> {
    current_shapes(doc, sel).map(|v| v.iter().map(|s| s.name.clone()).collect()).unwrap_or_default()
}

/// The command palette runs a command with `{}`. True when the params doc names a value that
/// call would have to supply (`path`, `rows`, `path? | data`), so the palette must not offer it.
/// A name ending in `?` is optional. Text after `→` is the result, not an input.
pub fn needs_arguments(params: &str) -> bool {
    let input = params.split('→').next().unwrap_or(params);
    let Some(start) = input.find('{') else { return false };
    let inside = input.get(start + 1..).unwrap_or("");
    let inside = inside.split('}').next().unwrap_or(inside);
    split_fields(inside).iter().any(|f| field_requires(f.trim()))
}

fn split_fields(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote = false;
    for (i, c) in s.char_indices() {
        if quote {
            if c == '"' {
                quote = false;
            }
            continue;
        }
        match c {
            '"' => quote = true,
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(s.get(start..i).unwrap_or(""));
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(s.get(start..).unwrap_or(""));
    out
}

/// A field needs a value unless every alternative (`path? | data`) is an optional parameter.
fn field_requires(field: &str) -> bool {
    if field.is_empty() {
        return false;
    }
    let alts: Vec<&str> = if field.contains(" | ") { field.split(" | ").map(str::trim).filter(|s| !s.is_empty()).collect() } else { vec![field] };
    let mut saw = false;
    let mut all_optional = true;
    for alt in alts {
        let Some(optional) = param_optional(alt) else { continue };
        saw = true;
        all_optional &= optional;
    }
    saw && !all_optional
}

/// `Some(true)` when `alt` starts with an optional parameter name, `Some(false)` when required.
fn param_optional(alt: &str) -> Option<bool> {
    let alt = alt.trim();
    let name_end = alt.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(alt.len());
    let name = alt.get(..name_end).unwrap_or("");
    if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let rest = alt.get(name_end..).unwrap_or("").trim_start();
    if rest.starts_with('?') {
        Some(true)
    } else if rest.is_empty() || rest.starts_with(':') {
        Some(false)
    } else {
        None
    }
}

/// Base64 (standard alphabet) for binary payloads in JSON params and results.
pub fn base64_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for (i, shift) in [18, 12, 6, 0].iter().enumerate() {
            if i <= chunk.len() {
                out.push(A.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A') as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.split_once("base64,").map(|(_, b)| b).unwrap_or(s);
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_skips_a_command_that_needs_a_value() {
        assert!(needs_arguments("{path}"));
        assert!(needs_arguments("{path, format: png|jpeg|pptx, slide?: index}"));
        assert!(needs_arguments("{path? | data: base64, name?, rect?}"));
        assert!(needs_arguments("{rows, cols, rect?}"));
        assert!(needs_arguments("{axis: horizontal|vertical, ids?}"));
        assert!(needs_arguments("{percent}"));
        assert!(!needs_arguments("{}"));
        assert!(!needs_arguments("{on?: bool}"));
        assert!(!needs_arguments("{theme?: name, size?: [w,h] pt, blank?: bool}"));
        assert!(!needs_arguments("{uid?: one entry, else all}"));
        assert!(!needs_arguments("{locked?: bool (default: toggle), ids?}"));
        assert!(!needs_arguments("{} → [{name, label, category}]"));
        assert!(!needs_arguments("{title?, subject?} → properties"));
        assert!(!needs_arguments("{slides?: [index] | \"1-3,5\", includeHidden?}"));
        let export = command_specs().iter().find(|c| c.id == "file.export").expect("export");
        let save = command_specs().iter().find(|c| c.id == "file.save").expect("save");
        let audio = command_specs().iter().find(|c| c.id == "insert.audio").expect("audio");
        assert!(needs_arguments(export.params));
        assert!(!needs_arguments(save.params));
        assert!(needs_arguments(audio.params));
    }

    #[test]
    fn preset_and_system_colors_keep_their_base() {
        use deckcraft_color::{ColorTransform, Rgba};
        use deckcraft_model::{ColorBase, ColorRef};
        use serde_json::json;
        let preset = color_value(&json!({"preset": "red", "lumMod": 50_000}));
        assert_eq!(preset, Some(ColorRef { base: ColorBase::Preset { name: "red".into() }, mods: vec![ColorTransform::LumMod(50_000)] }));
        let system = color_value(&json!({"system": "windowText", "last": "#123456"}));
        assert_eq!(system, Some(ColorRef { base: ColorBase::System { name: "windowText".into(), last: Rgba::rgb(0x12, 0x34, 0x56) }, mods: vec![] }));
        assert_eq!(color_value(&json!({"system": "windowText"})), None, "a system color needs its last value");
        assert_eq!(color_value(&json!({"preset": "notAColor"})), None);
        assert_eq!(color_value(&json!({"system": "notAColor", "last": "#000000"})), None);
        for name in ["aliceBlue", "darkBlue", "ltSlateGrey"] {
            assert!(color_value(&json!({"preset": name})).is_some(), "{name}");
        }
        assert!(color_value(&json!({"system": "3dDkShadow", "last": "#000000"})).is_some());
    }
}
