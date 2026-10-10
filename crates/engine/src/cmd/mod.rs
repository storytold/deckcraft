//! The command registry. Ids follow PowerPoint's ribbon and menu structure.

pub mod animation;
pub mod arrange;
pub mod design;
pub mod edit;
pub mod file;
pub mod format;
pub mod insert;
pub mod inspect;
pub mod media;
pub mod merge;
pub mod review;
pub mod shape;
pub mod slide;
pub mod table;
pub mod text;
pub mod transition;

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
    if s.active().is_some_and(|d| d.selection.text.is_some()) { Ok(()) } else { Err("no text insertion point".into()) }
}
/// Text editing or shapes with text selected (formatting commands apply to either).
pub fn has_text_or_shapes(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if s.active().is_some_and(|d| d.selection.text.is_some() || !d.selection.shapes.is_empty()) {
        Ok(())
    } else {
        Err("select text or a shape".into())
    }
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
        v.extend(insert::specs());
        v.extend(shape::specs());
        v.extend(arrange::specs());
        v.extend(merge::specs());
        v.extend(text::specs());
        v.extend(format::specs());
        v.extend(design::specs());
        v.extend(transition::specs());
        v.extend(animation::specs());
        v.extend(table::specs());
        v.extend(review::specs());
        v.extend(inspect::specs());
        v.extend(media::specs());
        v
    })
}

pub fn find_command(id: &str) -> Option<&'static CommandSpec> {
    command_specs().iter().find(|c| c.id == id)
}

// ---------- param helpers ----------

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

pub(crate) fn current_shapes<'a>(doc: &'a deckcraft_model::Presentation, sel: &crate::Selection) -> Option<&'a Vec<deckcraft_model::Shape>> {
    match sel.target {
        crate::Target::Slides => doc.slides.get(sel.slide).map(|s| &s.shapes),
        crate::Target::Master { master } => doc.masters.get(master).map(|m| &m.shapes),
        crate::Target::Layout { master, layout } => doc.masters.get(master).and_then(|m| m.layouts.get(layout)).map(|l| &l.shapes),
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
