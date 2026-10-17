//! Office add-ins' bridge (`office.sync`): one `context.sync()` of an add-in's task pane — a batch
//! of [`Action`]s on objects reached by paths from the presentation — run against the open
//! presentation. It is one undoable step; if any action fails, the presentation goes back to how it
//! was and the error names the action. What an add-in may do is bounded by its manifest's
//! permissions. Macro add-ins reach the same objects (`host/sync`, [`super::developer`]).
//!
//! The objects follow the PowerPoint JavaScript API's public reference: `presentation`, `slides`,
//! `slide`, `shapes`, `shape`, `textFrame`, `textRange`, `font`, `fill`, `lineFormat`, `tags`, the
//! selection, `insertSlidesFromBase64`, and the common API's `document` (selected text or slide
//! range, an image put in, `goToById`) and settings (docs/officejs.md). Colours are `#RRGGBB` or CSS
//! colour names. Writes go
//! through DeckCraft's own commands (`slide.new`, `shape.setBounds`, `text.set`, `format.bold`…), on
//! the slide the object is on; reads resolve placeholders, theme colours and fonts as they're drawn.

use std::collections::HashMap;

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::TextBody;
use deckcraft_model::{Fill, Shape, ShapeId, ShapeKind, Slide, SlideId};
use deckcraft_plugin::office::{Action, MAX_ACTIONS, Permission, Step};
use serde_json::{Value as Json, json};

use super::{CommandSpec, always, bad, cmd, str_param};
use crate::{EngineError, Result, Session, Target};

mod colors;

/// Most items a collection load answers.
const MAX_ITEMS: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "office.sync",
        "Office Add-in",
        [],
        None,
        "{addin, actions: [{op: load|set|call, path: [{p}|{m, a}], props?|prop+value|method+args?}]} → {results} or {error: {code, message, action}}. One context.sync() of an Office add-in, as one undoable step",
        always,
        sync
    )]
}

/// An object an action reaches.
#[derive(Clone, Debug, PartialEq)]
enum Obj {
    Root,
    Presentation,
    /// All slides, or the selected ones (`getSelectedSlides()`).
    Slides {
        selected: bool,
    },
    Slide(SlideId),
    Layout(SlideId),
    Master(SlideId),
    /// `presentation.slideMasters`, one of them by position, its layouts, one of those.
    Masters,
    MasterAt(usize),
    Layouts(usize),
    LayoutAt(usize, usize),
    /// A slide's shapes, or the selected shapes of the current slide (`getSelectedShapes()`).
    Shapes {
        slide: SlideId,
        selected: bool,
    },
    Shape(SlideId, ShapeId),
    TextFrame(SlideId, ShapeId),
    /// The shape's text from `start`, `len` characters (`None`: to the end).
    TextRange(SlideId, ShapeId, usize, Option<usize>),
    Font(SlideId, ShapeId),
    Fill(SlideId, ShapeId),
    Line(SlideId, ShapeId),
    /// The add-in tags of the presentation, a slide or a shape (`tags`), and one by key.
    Tags(Owner),
    Tag(Owner, String),
    /// The common API's `Office.context.document` (the selection as text).
    Document,
    /// `Office.context.document.settings`.
    Settings,
}

/// What a [`Obj::Tags`] collection belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Presentation,
    Slide(SlideId),
    Shape(SlideId, ShapeId),
}

/// Most tags one object keeps, and the longest key and value.
const MAX_TAGS: usize = 10_000;
const MAX_TAG_KEY: usize = 255;
const MAX_TAG_VALUE: usize = 1 << 20;

/// An Office.js error: its code (`ItemNotFound`, `InvalidArgument`, `AccessDenied`…) and message.
struct OfficeError(&'static str, String);

type R<T> = std::result::Result<T, OfficeError>;

fn invalid(msg: impl Into<String>) -> OfficeError {
    OfficeError("InvalidArgument", msg.into())
}

fn not_found(msg: impl Into<String>) -> OfficeError {
    OfficeError("ItemNotFound", msg.into())
}

fn engine(e: EngineError) -> OfficeError {
    OfficeError("GeneralException", e.to_string())
}

fn no_doc() -> OfficeError {
    OfficeError("GeneralException", "no presentation is open".into())
}

/// Add-in settings, per open presentation and add-in, until they're saved in the file.
fn settings_store() -> &'static std::sync::Mutex<HashMap<(u64, String), Json>> {
    static S: std::sync::OnceLock<std::sync::Mutex<HashMap<(u64, String), Json>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

fn sync(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "office.sync";
    let addin = str_param(p, "addin").ok_or_else(|| bad(ID, "missing `addin`"))?;
    let plugin = deckcraft_plugin::get(addin).ok_or_else(|| bad(ID, format!("no add-in `{addin}` is loaded")))?;
    let office = plugin.manifest.office.clone().ok_or_else(|| bad(ID, format!("{addin} isn't an Office add-in")))?;
    if !plugin.enabled() {
        return Err(bad(ID, format!("{addin} is turned off")));
    }
    if let Some(b) = plugin.blocked_by().first() {
        return Err(bad(ID, format!("{addin} is blocked: {}", b.message)));
    }
    let actions: Vec<Action> =
        serde_json::from_value(p.get("actions").cloned().unwrap_or(Json::Array(Vec::new()))).map_err(|e| bad(ID, format!("`actions`: {e}")))?;
    if actions.len() > MAX_ACTIONS {
        return Err(bad(ID, format!("at most {MAX_ACTIONS} actions in one sync")));
    }
    // One step: the commands run inside are no steps of their own, and a failure puts the
    // presentation back.
    let before = super::plugins::snapshot(s);
    match run_actions(s, addin, office.permissions, &actions) {
        Ok(results) => Ok(json!({"results": results})),
        // A failed sync is an answer the task pane reads (the promise rejects), not a command error.
        Err(error) => {
            super::plugins::restore(s, before);
            Ok(json!({"error": error}))
        }
    }
}

/// Runs `actions` in order for the add-in or plugin `addin`: their results, or the first failure
/// as `{code, message, action}`. The caller makes them one step (and takes them back on failure).
pub(crate) fn run_actions(s: &mut Session, addin: &str, perm: Permission, actions: &[Action]) -> std::result::Result<Vec<Json>, Json> {
    let mut results = Vec::with_capacity(actions.len());
    for (i, a) in actions.iter().enumerate() {
        // An object added earlier in the same sync is named by that action's result
        // (`getItem({"result": 2})`) until the task pane learns its id.
        let a = with_results(a, &results);
        match run_action(s, addin, perm, &a) {
            Ok(v) => results.push(v),
            Err(OfficeError(code, message)) => return Err(json!({"code": code, "message": message, "action": i})),
        }
    }
    Ok(results)
}

/// `a` with `{"result": i}` arguments of its path's steps replaced by the id action `i` answered
/// (`""`, which names nothing, when it answered none).
fn with_results(a: &Action, results: &[Json]) -> Action {
    let fix = |path: &[Step]| -> Vec<Step> {
        path.iter()
            .map(|step| match step {
                Step::Method { m, a } => Step::Method {
                    m: m.clone(),
                    a: a.iter()
                        .map(|v| match v.get("result").and_then(Json::as_u64).filter(|_| v.as_object().is_some_and(|o| o.len() == 1)) {
                            Some(i) => results.get(i as usize).and_then(|r| r.get("id")).cloned().unwrap_or(json!("")),
                            None => v.clone(),
                        })
                        .collect(),
                },
                other => other.clone(),
            })
            .collect()
    };
    match a {
        Action::Load { path, props } => Action::Load { path: fix(path), props: props.clone() },
        Action::Set { path, prop, value } => Action::Set { path: fix(path), prop: prop.clone(), value: value.clone() },
        Action::Call { path, method, args } => Action::Call { path: fix(path), method: method.clone(), args: args.clone() },
    }
}

fn need_read(perm: Permission) -> R<()> {
    if perm.reads() { Ok(()) } else { Err(OfficeError("AccessDenied", "the add-in's manifest doesn't let it read the presentation".into())) }
}

fn need_write(perm: Permission) -> R<()> {
    if perm.writes() { Ok(()) } else { Err(OfficeError("AccessDenied", "the add-in's manifest doesn't let it change the presentation".into())) }
}

fn run_action(s: &mut Session, addin: &str, perm: Permission, a: &Action) -> R<Json> {
    match a {
        Action::Load { path, props } => {
            let obj = resolve_path(s, path)?;
            if !matches!(obj, Obj::Settings) {
                need_read(perm)?;
            }
            load(s, addin, &obj, props)
        }
        Action::Set { path, prop, value } => {
            let obj = resolve_path(s, path)?;
            need_write(perm)?;
            set(s, &obj, prop, value).map(|_| Json::Null)
        }
        Action::Call { path, method, args } => {
            let obj = resolve_path(s, path)?;
            match (&obj, method.as_str()) {
                (Obj::Settings, _) => {}
                (Obj::Document, "getSelectedData" | "goToById") => need_read(perm)?,
                _ => need_write(perm)?,
            }
            call(s, addin, &obj, method, args)
        }
    }
}

// ---------------------------------------------------------------- helpers shared with plugins

/// A slide's title text (its title placeholder), as add-ins and plugin commands see it.
pub(crate) fn slide_title(sl: &Slide) -> String {
    sl.title()
}

/// The text between two (paragraph, char) positions, paragraphs joined with `\n`.
pub(crate) fn text_between(body: &TextBody, from: (usize, usize), to: (usize, usize)) -> String {
    let mut out = String::new();
    for (pi, para) in body.paragraphs.iter().enumerate().skip(from.0).take(to.0.saturating_sub(from.0) + 1) {
        let text: Vec<char> = para.text().chars().collect();
        let start = if pi == from.0 { from.1 } else { 0 };
        let end = if pi == to.0 { to.1 } else { text.len() };
        if pi > from.0 {
            out.push('\n');
        }
        out.extend(text.iter().skip(start).take(end.saturating_sub(start)));
    }
    out
}

// ---------------------------------------------------------------- paths

fn arg_str(args: &[Json], i: usize) -> Option<&str> {
    args.get(i).and_then(Json::as_str)
}

fn arg_index(args: &[Json], i: usize) -> R<usize> {
    args.get(i)
        .and_then(|v| v.as_u64().or_else(|| v.as_f64().filter(|f| f.is_finite() && *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64)))
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| invalid(format!("argument {} must be a whole number of 0 or more", i + 1)))
}

/// An object id given as `"257"` or `257`.
fn arg_id(args: &[Json], i: usize) -> R<u32> {
    let v = args.get(i).ok_or_else(|| invalid(format!("argument {} (an id) is missing", i + 1)))?;
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| v.as_str().and_then(|t| t.trim().trim_end_matches('#').parse::<u32>().ok()))
        .ok_or_else(|| invalid(format!("`{v}` isn't an id")))
}

fn doc(s: &Session) -> R<&crate::DocState> {
    s.active().ok_or_else(no_doc)
}

fn slide_of(s: &Session, id: SlideId) -> R<&Slide> {
    doc(s)?.doc.slide(id).ok_or_else(|| not_found(format!("there's no slide {}", id.0)))
}

fn shape_of(s: &Session, slide: SlideId, id: ShapeId) -> R<&Shape> {
    slide_of(s, slide)?.shape(id).ok_or_else(|| not_found(format!("there's no shape {} on slide {}", id.0, slide.0)))
}

fn current_slide(s: &Session) -> R<SlideId> {
    doc(s)?.current_slide().map(|sl| sl.id).ok_or_else(|| not_found("the presentation has no slides"))
}

/// The slides a collection holds.
fn slide_ids(s: &Session, selected: bool) -> R<Vec<SlideId>> {
    let d = doc(s)?;
    if selected {
        if !d.selection.slides.is_empty() {
            return Ok(d.selection.slides.clone());
        }
        return Ok(d.current_slide().map(|sl| vec![sl.id]).unwrap_or_default());
    }
    Ok(d.doc.slides.iter().map(|sl| sl.id).collect())
}

/// The shapes a collection holds: a slide's top-level shapes, or the selected ones.
fn shape_ids(s: &Session, slide: SlideId, selected: bool) -> R<Vec<ShapeId>> {
    if selected {
        let d = doc(s)?;
        return Ok(if d.selection.target == Target::Slides { d.selection.shapes.clone() } else { Vec::new() });
    }
    Ok(slide_of(s, slide)?.shapes.iter().map(|sh| sh.id).collect())
}

fn resolve_path(s: &Session, path: &[Step]) -> R<Obj> {
    let mut obj = Obj::Root;
    for step in path {
        obj = match (&obj, step) {
            (Obj::Root, Step::Prop { p }) if p == "presentation" => Obj::Presentation,
            (Obj::Root, Step::Prop { p }) if p == "document" => Obj::Document,
            (Obj::Root, Step::Prop { p }) if p == "settings" => Obj::Settings,
            (Obj::Presentation, Step::Prop { p }) if p == "slides" => Obj::Slides { selected: false },
            (Obj::Presentation, Step::Method { m, .. }) if m == "getSelectedSlides" => Obj::Slides { selected: true },
            (Obj::Presentation, Step::Prop { p }) if p == "slideMasters" => Obj::Masters,
            (Obj::Masters, Step::Method { m, a }) if m == "getItemAt" || m == "getItem" => {
                let d = doc(s)?;
                let i = if m == "getItemAt" {
                    arg_index(a, 0)?
                } else {
                    let id = arg_id(a, 0)?;
                    d.doc.masters.iter().position(|ma| ma.id.0 == id).ok_or_else(|| not_found(format!("there's no slide master {id}")))?
                };
                if i >= d.doc.masters.len() {
                    return Err(not_found(format!("there's no slide master at index {i}")));
                }
                Obj::MasterAt(i)
            }
            (Obj::MasterAt(i), Step::Prop { p }) if p == "layouts" => Obj::Layouts(*i),
            (Obj::Layouts(mi), Step::Method { m, a }) if m == "getItemAt" || m == "getItem" => {
                let layouts = doc(s)?.doc.masters.get(*mi).map(|ma| ma.layouts.iter().map(|l| l.id.0).collect::<Vec<_>>()).unwrap_or_default();
                let i = if m == "getItemAt" {
                    arg_index(a, 0)?
                } else {
                    let id = arg_id(a, 0)?;
                    layouts.iter().position(|l| *l == id).ok_or_else(|| not_found(format!("there's no layout {id}")))?
                };
                if i >= layouts.len() {
                    return Err(not_found(format!("there's no layout at index {i}")));
                }
                Obj::LayoutAt(*mi, i)
            }
            (Obj::Presentation, Step::Method { m, .. }) if m == "getSelectedShapes" => Obj::Shapes { slide: current_slide(s)?, selected: true },
            (Obj::Slides { selected }, Step::Method { m, a }) if m == "getItem" => {
                let id = SlideId(arg_id(a, 0)?);
                if !slide_ids(s, *selected)?.contains(&id) {
                    return Err(not_found(format!("there's no slide {}", id.0)));
                }
                Obj::Slide(id)
            }
            (Obj::Slides { selected }, Step::Method { m, a }) if m == "getItemAt" => {
                let i = arg_index(a, 0)?;
                let ids = slide_ids(s, *selected)?;
                Obj::Slide(*ids.get(i).ok_or_else(|| not_found(format!("there's no slide at index {i} (there are {})", ids.len())))?)
            }
            (Obj::Slide(id), Step::Prop { p }) if p == "shapes" => Obj::Shapes { slide: *id, selected: false },
            (Obj::Slide(id), Step::Prop { p }) if p == "layout" => Obj::Layout(*id),
            (Obj::Slide(id), Step::Prop { p }) if p == "slideMaster" => Obj::Master(*id),
            (Obj::Shapes { slide, selected }, Step::Method { m, a }) if m == "getItem" => {
                let id = ShapeId(arg_id(a, 0)?);
                if !shape_ids(s, *slide, *selected)?.contains(&id) && !(!*selected && slide_of(s, *slide)?.shape(id).is_some()) {
                    return Err(not_found(format!("there's no shape {}", id.0)));
                }
                Obj::Shape(*slide, id)
            }
            (Obj::Shapes { slide, selected }, Step::Method { m, a }) if m == "getItemAt" => {
                let i = arg_index(a, 0)?;
                let ids = shape_ids(s, *slide, *selected)?;
                Obj::Shape(*slide, *ids.get(i).ok_or_else(|| not_found(format!("there's no shape at index {i} (there are {})", ids.len())))?)
            }
            (Obj::Shape(sl, id), Step::Prop { p }) if p == "textFrame" => {
                let sh = shape_of(s, *sl, *id)?;
                // Lines, pictures, groups, tables and the like have no text frame.
                if !matches!(sh.kind, ShapeKind::Shape) || sh.is_line() {
                    return Err(OfficeError("InvalidArgument", format!("shape {} has no text frame", id.0)));
                }
                Obj::TextFrame(*sl, *id)
            }
            (Obj::Shape(sl, id), Step::Prop { p }) if p == "fill" => Obj::Fill(*sl, *id),
            (Obj::Shape(sl, id), Step::Prop { p }) if p == "lineFormat" => Obj::Line(*sl, *id),
            (Obj::TextFrame(sl, id), Step::Prop { p }) if p == "textRange" => Obj::TextRange(*sl, *id, 0, None),
            (Obj::TextRange(sl, id, start, _), Step::Method { m, a }) if m == "getSubstring" => {
                let from = arg_index(a, 0)?;
                let len = if a.len() > 1 { Some(arg_index(a, 1)?) } else { None };
                Obj::TextRange(*sl, *id, start.saturating_add(from), len)
            }
            (Obj::TextRange(sl, id, ..), Step::Prop { p }) if p == "font" => Obj::Font(*sl, *id),
            (Obj::Presentation, Step::Prop { p }) if p == "tags" => Obj::Tags(Owner::Presentation),
            (Obj::Slide(id), Step::Prop { p }) if p == "tags" => Obj::Tags(Owner::Slide(*id)),
            (Obj::Shape(sl, id), Step::Prop { p }) if p == "tags" => Obj::Tags(Owner::Shape(*sl, *id)),
            (Obj::Tags(owner), Step::Method { m, a }) if m == "getItem" => {
                let key = arg_str(a, 0).ok_or_else(|| invalid("getItem needs a tag's key"))?;
                let tags = tags_of(s, *owner)?;
                let (k, _) =
                    tags.iter().find(|(k, _)| k.eq_ignore_ascii_case(key.trim())).ok_or_else(|| not_found(format!("there's no tag {key}")))?;
                Obj::Tag(*owner, k.clone())
            }
            (Obj::Tags(owner), Step::Method { m, a }) if m == "getItemAt" => {
                let i = arg_index(a, 0)?;
                let tags = tags_of(s, *owner)?;
                let (k, _) = tags.get(i).ok_or_else(|| not_found(format!("there's no tag at index {i} (there are {})", tags.len())))?;
                Obj::Tag(*owner, k.clone())
            }
            (o, step) => return Err(invalid(format!("{} has no {}", name(o), step_name(step)))),
        };
        // Every step lands on something that exists.
        match &obj {
            Obj::Slide(id) | Obj::Layout(id) | Obj::Master(id) => {
                slide_of(s, *id)?;
            }
            Obj::Shape(sl, id) | Obj::TextFrame(sl, id) | Obj::TextRange(sl, id, ..) | Obj::Font(sl, id) | Obj::Fill(sl, id) | Obj::Line(sl, id) => {
                shape_of(s, *sl, *id)?;
            }
            _ => {}
        }
    }
    Ok(obj)
}

fn name(o: &Obj) -> &'static str {
    match o {
        Obj::Root => "the add-in's context",
        Obj::Presentation => "presentation",
        Obj::Slides { .. } => "SlideCollection",
        Obj::Slide(_) => "Slide",
        Obj::Layout(_) => "SlideLayout",
        Obj::Master(_) | Obj::MasterAt(_) => "SlideMaster",
        Obj::Masters => "SlideMasterCollection",
        Obj::Layouts(_) => "SlideLayoutCollection",
        Obj::LayoutAt(..) => "SlideLayout",
        Obj::Shapes { .. } => "ShapeCollection",
        Obj::Shape(..) => "Shape",
        Obj::TextFrame(..) => "TextFrame",
        Obj::TextRange(..) => "TextRange",
        Obj::Font(..) => "ShapeFont",
        Obj::Fill(..) => "ShapeFill",
        Obj::Line(..) => "ShapeLineFormat",
        Obj::Tags(_) => "TagCollection",
        Obj::Tag(..) => "Tag",
        Obj::Document => "Document",
        Obj::Settings => "Settings",
    }
}

fn step_name(step: &Step) -> String {
    match step {
        Step::Prop { p } => format!("property `{p}`"),
        Step::Method { m, .. } => format!("method `{m}()`"),
    }
}

// ---------------------------------------------------------------- reading

fn hex(c: deckcraft_model::Rgba) -> Json {
    json!(format!("#{}", c.hex()))
}

/// The value every run of the shape's text has for `f`, or null when they differ.
fn common(values: Vec<Json>) -> Json {
    match values.split_first() {
        Some((first, rest)) if rest.iter().all(|v| v == first) => first.clone(),
        Some(_) => Json::Null,
        None => Json::Null,
    }
}

fn shape_type(sh: &Shape) -> &'static str {
    match &sh.kind {
        ShapeKind::Shape if sh.is_line() => "line",
        ShapeKind::Shape if sh.text_box => "textBox",
        ShapeKind::Shape if sh.ph.is_some() => "placeholder",
        ShapeKind::Shape => "geometricShape",
        ShapeKind::Connector { .. } => "line",
        ShapeKind::Picture { .. } => "image",
        ShapeKind::Group { .. } => "group",
        ShapeKind::Table(_) => "table",
        ShapeKind::Chart(_) => "chart",
        ShapeKind::Media(_) => "media",
        ShapeKind::Model3d { .. } => "model3D",
        ShapeKind::Ink { .. } | ShapeKind::Opaque { .. } => "unsupported",
    }
}

fn underline_name(u: Option<&str>) -> &str {
    match u {
        None | Some("none") => "none",
        Some("sng") => "single",
        Some("dbl") => "double",
        Some(other) => other,
    }
}

/// The shape's text as a TextRange sees it: (whole text, chars from start, length).
fn range_text(sh: &Shape, start: usize, len: Option<usize>) -> (String, usize, usize) {
    let all: Vec<char> = sh.text.as_ref().map(|t| t.text()).unwrap_or_default().chars().collect();
    let start = start.min(all.len());
    let len = len.unwrap_or(all.len() - start).min(all.len() - start);
    (all.iter().skip(start).take(len).collect(), start, len)
}

fn load(s: &mut Session, addin: &str, obj: &Obj, props: &[String]) -> R<Json> {
    if props.len() > 256 {
        return Err(invalid("at most 256 properties in one load"));
    }
    // Settings load whole: the add-in's saved object.
    if matches!(obj, Obj::Settings) {
        let uid = s.active().map(|d| d.uid).unwrap_or(0);
        let store = match settings_store().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        return Ok(store.get(&(uid, addin.to_string())).cloned().unwrap_or_else(|| json!({})));
    }
    let mut out = serde_json::Map::new();
    for prop in props {
        let v = match (obj, prop.as_str()) {
            (Obj::Presentation, "title") => json!(doc(s)?.title()),
            (Obj::Slides { selected }, "count") => json!(slide_ids(s, *selected)?.len()),
            (Obj::Shapes { slide, selected }, "count") => json!(shape_ids(s, *slide, *selected)?.len()),
            (Obj::Slides { selected }, "items") => {
                let rest: Vec<String> = props.iter().filter(|p| *p != "items" && *p != "count").cloned().collect();
                let ids = slide_ids(s, *selected)?;
                let mut items = Vec::new();
                for id in ids.into_iter().take(MAX_ITEMS) {
                    let mut item = load(s, addin, &Obj::Slide(id), &rest)?;
                    item["id"] = json!(id.0.to_string());
                    items.push(item);
                }
                Json::Array(items)
            }
            (Obj::Shapes { slide, selected }, "items") => {
                let rest: Vec<String> = props.iter().filter(|p| *p != "items" && *p != "count").cloned().collect();
                let ids = shape_ids(s, *slide, *selected)?;
                let mut items = Vec::new();
                for id in ids.into_iter().take(MAX_ITEMS) {
                    let mut item = load(s, addin, &Obj::Shape(*slide, id), &rest)?;
                    item["id"] = json!(id.0.to_string());
                    items.push(item);
                }
                Json::Array(items)
            }
            // Other props listed with `items` are the items'.
            (Obj::Slides { .. } | Obj::Shapes { .. }, _) if props.iter().any(|p| p == "items") => continue,
            (Obj::Masters, "count") => json!(doc(s)?.doc.masters.len()),
            (Obj::Layouts(mi), "count") => json!(doc(s)?.doc.masters.get(*mi).map_or(0, |m| m.layouts.len())),
            (Obj::Masters, "items") => {
                let d = doc(s)?;
                Json::Array(d.doc.masters.iter().take(MAX_ITEMS).map(|m| json!({"id": m.id.0.to_string(), "name": m.name})).collect())
            }
            (Obj::Layouts(mi), "items") => {
                let d = doc(s)?;
                let layouts = d.doc.masters.get(*mi).map(|m| m.layouts.as_slice()).unwrap_or(&[]);
                Json::Array(layouts.iter().take(MAX_ITEMS).map(|l| json!({"id": l.id.0.to_string(), "name": l.name})).collect())
            }
            (Obj::Masters | Obj::Layouts(_), _) if props.iter().any(|p| p == "items") => continue,
            (Obj::Tags(owner), "count") => json!(tags_of(s, *owner)?.len()),
            (Obj::Tags(owner), "items") => {
                Json::Array(tags_of(s, *owner)?.iter().take(MAX_ITEMS).map(|(k, v)| json!({"key": k, "value": v})).collect())
            }
            (Obj::Tags(_), _) if props.iter().any(|p| p == "items") => continue,
            (Obj::Tag(owner, key), p @ ("key" | "value")) => {
                let tags = tags_of(s, *owner)?;
                let (k, v) = tags.iter().find(|(k, _)| k == key).ok_or_else(|| not_found(format!("there's no tag {key}")))?;
                if p == "key" { json!(k) } else { json!(v) }
            }
            (Obj::MasterAt(i), p @ ("id" | "name")) => {
                let m = doc(s)?.doc.masters.get(*i).ok_or_else(|| not_found("no such slide master"))?;
                if p == "id" { json!(m.id.0.to_string()) } else { json!(m.name) }
            }
            (Obj::LayoutAt(mi, li), p @ ("id" | "name")) => {
                let l = doc(s)?.doc.masters.get(*mi).and_then(|m| m.layouts.get(*li)).ok_or_else(|| not_found("no such layout"))?;
                if p == "id" { json!(l.id.0.to_string()) } else { json!(l.name) }
            }
            (Obj::Slide(id), "id") => json!(id.0.to_string()),
            (Obj::Slide(id), "index") => json!(doc(s)?.doc.slide_index(*id)),
            (Obj::Layout(id), p @ ("id" | "name")) => {
                let d = doc(s)?;
                let sl = slide_of(s, *id)?;
                let l = d.doc.layout(sl.layout).map(|(_, l)| l);
                if p == "id" { json!(sl.layout.0.to_string()) } else { json!(l.map(|l| l.name.clone()).unwrap_or_default()) }
            }
            (Obj::Master(id), p @ ("id" | "name")) => {
                let d = doc(s)?;
                let sl = slide_of(s, *id)?;
                let m = d.doc.master_for(sl).map(|(m, _)| m);
                if p == "id" { json!(m.map(|m| m.id.0.to_string())) } else { json!(m.map(|m| m.name.clone()).unwrap_or_default()) }
            }
            (Obj::Shape(sl, id), p) => {
                let d = doc(s)?;
                let slide = slide_of(s, *sl)?;
                let sh = shape_of(s, *sl, *id)?;
                let x = Ctx::for_slide(&d.doc, slide).map(|c| resolve::xfrm(&c, sh)).or(sh.xfrm).unwrap_or_default();
                match p {
                    "id" => json!(id.0.to_string()),
                    "name" => json!(sh.name),
                    "type" => json!(shape_type(sh)),
                    "left" => json!(x.x),
                    "top" => json!(x.y),
                    "width" => json!(x.w),
                    "height" => json!(x.h),
                    _ => return Err(invalid(format!("Shape has no property `{p}`"))),
                }
            }
            (Obj::TextFrame(sl, id), p) => {
                let d = doc(s)?;
                let slide = slide_of(s, *sl)?;
                let sh = shape_of(s, *sl, *id)?;
                let body = Ctx::for_slide(&d.doc, slide).map(|c| resolve::body(&c, sh));
                match p {
                    "hasText" => json!(sh.text.as_ref().is_some_and(|t| !t.is_empty())),
                    "wordWrap" => json!(body.as_ref().and_then(|b| b.wrap).unwrap_or(!sh.text_box)),
                    "autoSizeSetting" => json!(match body.as_ref().and_then(|b| b.autofit) {
                        Some(deckcraft_model::text::AutoFit::Shape) => "autoSizeShapeToFitText",
                        Some(deckcraft_model::text::AutoFit::Shrink { .. }) => "autoSizeTextToFitShape",
                        _ => "autoSizeNone",
                    }),
                    _ => return Err(invalid(format!("TextFrame has no property `{p}`"))),
                }
            }
            (Obj::TextRange(sl, id, start, len), p) => {
                let (text, from, n) = range_text(shape_of(s, *sl, *id)?, *start, *len);
                match p {
                    "text" => json!(text),
                    "start" => json!(from),
                    "length" => json!(n),
                    _ => return Err(invalid(format!("TextRange has no property `{p}`"))),
                }
            }
            (Obj::Font(sl, id), p) => {
                let d = doc(s)?;
                let slide = slide_of(s, *sl)?;
                let sh = shape_of(s, *sl, *id)?;
                let Some(ctx) = Ctx::for_slide(&d.doc, slide) else { return Err(OfficeError("GeneralException", "the slide has no master".into())) };
                let mut runs = Vec::new();
                for para in sh.text.as_ref().map(|t| t.paragraphs.as_slice()).unwrap_or(&[]).iter().take(MAX_ITEMS) {
                    if para.runs.is_empty() {
                        runs.push(resolve::run(&ctx, sh, para, &para.end_props));
                    }
                    for r in para.runs.iter().take(MAX_ITEMS) {
                        runs.push(resolve::run(&ctx, sh, para, &r.props));
                    }
                }
                let vals: Vec<Json> = runs
                    .iter()
                    .map(|r| match p {
                        "bold" => json!(r.bold.unwrap_or(false)),
                        "italic" => json!(r.italic.unwrap_or(false)),
                        "underline" => json!(underline_name(r.underline.as_deref())),
                        "size" => json!(r.size.unwrap_or(18.0)),
                        "name" => json!(resolve::font_family(&ctx, r)),
                        "color" => hex(resolve::text_color(&ctx, r)),
                        _ => Json::Bool(false),
                    })
                    .collect();
                if !matches!(p, "bold" | "italic" | "underline" | "size" | "name" | "color") {
                    return Err(invalid(format!("ShapeFont has no property `{p}`")));
                }
                common(vals)
            }
            (Obj::Fill(sl, id), p) => {
                let d = doc(s)?;
                let slide = slide_of(s, *sl)?;
                let sh = shape_of(s, *sl, *id)?;
                let ctx = Ctx::for_slide(&d.doc, slide);
                let (fill, ph) = ctx.as_ref().map(|c| resolve::fill(c, sh)).unwrap_or((sh.fill.clone(), None));
                let color = match (&fill, &ctx) {
                    (Some(Fill::Solid { color }), Some(c)) => Some(c.color(color, ph)),
                    _ => None,
                };
                match p {
                    "type" => json!(match fill {
                        None | Some(Fill::None) => "noFill",
                        Some(Fill::Solid { .. }) => "solid",
                        Some(Fill::Gradient(_)) => "gradient",
                        Some(Fill::Picture(_)) => "pictureAndTexture",
                        Some(Fill::Pattern(_)) => "pattern",
                        Some(_) => "unsupported",
                    }),
                    "foregroundColor" => color.map(hex).unwrap_or(Json::Null),
                    "transparency" => json!(color.map(|c| ((255.0 - f64::from(c.a)) / 255.0 * 100.0).round() / 100.0).unwrap_or(0.0)),
                    _ => return Err(invalid(format!("ShapeFill has no property `{p}`"))),
                }
            }
            (Obj::Line(sl, id), p) => {
                let d = doc(s)?;
                let slide = slide_of(s, *sl)?;
                let sh = shape_of(s, *sl, *id)?;
                let ctx = Ctx::for_slide(&d.doc, slide);
                let (line, ph) = ctx.as_ref().map(|c| resolve::line(c, sh)).unwrap_or((sh.line.clone().unwrap_or_default(), None));
                let visible = !matches!(line.fill, None | Some(Fill::None));
                match p {
                    "visible" => json!(visible),
                    "color" => match (&line.fill, &ctx) {
                        (Some(Fill::Solid { color }), Some(c)) => hex(c.color(color, ph)),
                        _ => Json::Null,
                    },
                    "weight" => json!(line.width.unwrap_or(0.75)),
                    "dashStyle" => serde_json::to_value(&line.dash).ok().filter(|v| !v.is_null()).unwrap_or(json!("solid")),
                    _ => return Err(invalid(format!("ShapeLineFormat has no property `{p}`"))),
                }
            }
            (o, p) => return Err(invalid(format!("{} has no property `{p}`", name(o)))),
        };
        out.insert(prop.clone(), v);
    }
    Ok(Json::Object(out))
}

// ---------------------------------------------------------------- writing

fn run(s: &mut Session, id: &str, params: Json) -> R<Json> {
    s.execute(id, &params).map_err(engine)
}

/// Runs `f` with slide `slide` current and `shapes` selected (no text editing), then puts the
/// current slide and the selection back (selection changes are no undo steps).
fn on<T>(s: &mut Session, slide: SlideId, shapes: Vec<ShapeId>, f: impl FnOnce(&mut Session) -> R<T>) -> R<T> {
    let d = doc(s)?;
    let saved = d.selection.clone();
    let saved_slide = d.current_slide().map(|sl| sl.id);
    let index = d.doc.slide_index(slide).ok_or_else(|| not_found(format!("there's no slide {}", slide.0)))?;
    s.select(|_, sel| {
        sel.target = Target::Slides;
        sel.slide = index;
        sel.shapes = shapes;
        sel.text = None;
        sel.cells = None;
    })
    .map_err(engine)?;
    let r = f(s);
    let _ = s.select(|doc, sel| {
        let back = saved_slide.and_then(|id| doc.slide_index(id)).unwrap_or(saved.slide);
        *sel = saved;
        sel.slide = back;
    });
    r
}

fn num(value: &Json, what: &str) -> R<f64> {
    value.as_f64().filter(|v| v.is_finite()).ok_or_else(|| invalid(format!("{what} is a number")))
}

/// A colour as add-ins give it: `#RRGGBB` or a CSS colour name (`"red"`), any case.
fn color(value: &Json) -> R<String> {
    let c = value.as_str().map(str::trim).filter(|c| !c.is_empty()).ok_or_else(|| invalid("a colour is a string like #FF0000 or red"))?;
    colors::parse(c).ok_or_else(|| invalid(format!("`{}` isn't a colour like #FF0000 or red", c.chars().take(40).collect::<String>())))
}

fn set(s: &mut Session, obj: &Obj, prop: &str, value: &Json) -> R<()> {
    match (obj, prop) {
        (Obj::Shape(sl, id), "name") => {
            let name = value.as_str().ok_or_else(|| invalid("a name is text"))?.to_string();
            on(s, *sl, vec![*id], |s| run(s, "shape.rename", json!({"name": name, "id": id.0})))?;
        }
        (Obj::Shape(sl, id), p @ ("left" | "top" | "width" | "height")) => {
            let v = num(value, p)?;
            if matches!(p, "width" | "height") && v < 0.0 {
                return Err(invalid(format!("{p} can't be negative")));
            }
            let mut x = load(s, "", obj, &["left".into(), "top".into(), "width".into(), "height".into()])?;
            x[p] = json!(v);
            let g = |k: &str| x.get(k).and_then(Json::as_f64).unwrap_or(0.0);
            let params = json!({"x": g("left"), "y": g("top"), "w": g("width"), "h": g("height"), "ids": [id.0]});
            on(s, *sl, vec![*id], |s| run(s, "shape.setBounds", params))?;
        }
        (Obj::TextRange(sl, id, start, len), "text") => {
            let text = value.as_str().ok_or_else(|| invalid("text is a string"))?;
            if text.chars().count() > 1_000_000 {
                return Err(invalid("text is limited to a million characters"));
            }
            let sh = shape_of(s, *sl, *id)?;
            let whole: String = sh.text.as_ref().map(|t| t.text()).unwrap_or_default();
            let (_, from, n) = range_text(sh, *start, *len);
            // A part of the text: the whole text with that part replaced.
            let new: String = if from == 0 && n == whole.chars().count() {
                text.to_string()
            } else {
                whole.chars().take(from).chain(text.chars()).chain(whole.chars().skip(from + n)).collect()
            };
            on(s, *sl, vec![*id], |s| run(s, "text.set", json!({"id": id.0, "text": new})))?;
        }
        (Obj::TextFrame(sl, id), "wordWrap") => {
            let on_ = value.as_bool().ok_or_else(|| invalid("wordWrap is true or false"))?;
            on(s, *sl, vec![*id], |s| run(s, "format.wrap", json!({"on": on_})))?;
        }
        (Obj::TextFrame(sl, id), "autoSizeSetting") => {
            let mode = match value.as_str() {
                Some("autoSizeNone") => "none",
                Some("autoSizeTextToFitShape") => "shrink",
                Some("autoSizeShapeToFitText") => "resize",
                _ => return Err(invalid("autoSizeSetting is autoSizeNone, autoSizeTextToFitShape or autoSizeShapeToFitText")),
            };
            on(s, *sl, vec![*id], |s| run(s, "format.autofit", json!({"mode": mode})))?;
        }
        (Obj::Font(sl, id), p) => {
            let (cmd, params) = match p {
                "bold" | "italic" => (
                    if p == "bold" { "format.bold" } else { "format.italic" },
                    json!({"on": value.as_bool().ok_or_else(|| invalid(format!("{p} is true or false")))?}),
                ),
                "underline" => match value.as_str() {
                    Some("none") => ("format.underline", json!({"on": false})),
                    Some("single") => ("format.underline", json!({"on": true, "style": "sng"})),
                    Some("double") => ("format.underline", json!({"on": true, "style": "dbl"})),
                    Some(style @ ("heavy" | "dotted" | "dash" | "wavy")) => ("format.underline", json!({"on": true, "style": style})),
                    _ => return Err(invalid("underline is none, single, double, heavy, dotted, dash or wavy")),
                },
                "size" => {
                    let v = num(value, "size")?;
                    if !(1.0..=4000.0).contains(&v) {
                        return Err(invalid("size is 1 to 4000 points"));
                    }
                    ("format.size", json!({"size": v}))
                }
                "name" => (
                    "format.font",
                    json!({"family": value.as_str().filter(|n| !n.trim().is_empty()).ok_or_else(|| invalid("name is a font's name"))?}),
                ),
                "color" => ("format.color", json!({"color": color(value)?})),
                _ => return Err(invalid(format!("ShapeFont has no property `{p}`"))),
            };
            on(s, *sl, vec![*id], |s| run(s, cmd, params))?;
        }
        (Obj::Fill(sl, id), "foregroundColor") => {
            let c = color(value)?;
            on(s, *sl, vec![*id], |s| run(s, "shape.fill", json!({"color": c, "ids": [id.0]})))?;
        }
        (Obj::Fill(sl, id), "transparency") => {
            let t = num(value, "transparency")?;
            if !(0.0..=1.0).contains(&t) {
                return Err(invalid("transparency is 0 to 1"));
            }
            on(s, *sl, vec![*id], |s| run(s, "shape.fill", json!({"transparency": t, "ids": [id.0]})))?;
        }
        (Obj::Line(sl, id), p) => {
            let params = match p {
                "color" => json!({"color": color(value)?}),
                "weight" => {
                    let w = num(value, "weight")?;
                    if !(0.0..=1584.0).contains(&w) {
                        return Err(invalid("weight is 0 to 1584 points"));
                    }
                    json!({"width": w})
                }
                "visible" => {
                    if value.as_bool().ok_or_else(|| invalid("visible is true or false"))? {
                        // Shown again: the line it would have by default.
                        json!({"reset": true})
                    } else {
                        json!({"none": true})
                    }
                }
                "dashStyle" => json!({"dash": value.as_str().ok_or_else(|| invalid("dashStyle is a name like dash"))?}),
                _ => return Err(invalid(format!("ShapeLineFormat has no property `{p}`"))),
            };
            let mut params = params;
            params["ids"] = json!([id.0]);
            on(s, *sl, vec![*id], |s| run(s, "shape.line", params))?;
        }
        (Obj::Tag(owner, key), "value") => {
            let v = tag_value(value)?;
            let key = key.clone();
            edit_tags(s, *owner, |tags| {
                if let Some(t) = tags.iter_mut().find(|(k, _)| *k == key) {
                    t.1 = v;
                }
                Ok(())
            })?;
        }
        (o, p) => return Err(invalid(format!("{}.{p} can't be set", name(o)))),
    }
    Ok(())
}

// ---------------------------------------------------------------- tags

/// The tags of the presentation, a slide or a shape.
fn tags_of(s: &Session, owner: Owner) -> R<&Vec<(String, String)>> {
    match owner {
        Owner::Presentation => Ok(&doc(s)?.doc.tags),
        Owner::Slide(id) => Ok(&slide_of(s, id)?.tags),
        Owner::Shape(sl, id) => Ok(&shape_of(s, sl, id)?.tags),
    }
}

/// Changes the tags of the presentation, a slide or a shape.
fn edit_tags(s: &mut Session, owner: Owner, f: impl FnOnce(&mut Vec<(String, String)>) -> R<()>) -> R<()> {
    tags_of(s, owner)?;
    let mut failure = None;
    s.edit(|d, _| {
        let tags = match owner {
            Owner::Presentation => Some(&mut d.tags),
            Owner::Slide(id) => d.slide_mut(id).map(|sl| &mut sl.tags),
            Owner::Shape(sl, id) => d.slide_mut(sl).and_then(|sl| sl.shape_mut(id)).map(|sh| &mut sh.tags),
        };
        match tags {
            Some(t) => failure = f(t).err(),
            None => failure = Some(not_found("the tags' slide or shape is gone")),
        }
        Ok(())
    })
    .map_err(engine)?;
    failure.map_or(Ok(()), Err)
}

/// A tag's key as PowerPoint keeps it: upper case.
fn tag_key(v: Option<&Json>) -> R<String> {
    let k = v.and_then(Json::as_str).map(str::trim).filter(|k| !k.is_empty()).ok_or_else(|| invalid("a tag's key is a non-empty string"))?;
    if k.chars().count() > MAX_TAG_KEY || k.chars().any(char::is_control) {
        return Err(invalid(format!("a tag's key is at most {MAX_TAG_KEY} characters, without control characters")));
    }
    Ok(k.to_uppercase())
}

fn tag_value(v: &Json) -> R<String> {
    let v = v.as_str().ok_or_else(|| invalid("a tag's value is a string"))?;
    if v.len() > MAX_TAG_VALUE {
        return Err(invalid("a tag's value is limited to 1 MB"));
    }
    Ok(v.to_string())
}

/// `{left, top, width, height}` of an `add…` call's options (points), with defaults.
fn bounds(args: &[Json], i: usize) -> R<[f64; 4]> {
    let o = args.get(i).cloned().unwrap_or(Json::Null);
    let g = |k: &str, d: f64| -> R<f64> {
        match o.get(k) {
            None | Some(Json::Null) => Ok(d),
            Some(v) => v.as_f64().filter(|v| v.is_finite()).ok_or_else(|| invalid(format!("`{k}` is a number of points"))),
        }
    };
    Ok([g("left", 100.0)?, g("top", 100.0)?, g("width", 200.0)?.max(0.0), g("height", 100.0)?.max(0.0)])
}

/// The PowerPoint JS geometric shape type names, as DeckCraft presets.
fn preset_of(kind: &str) -> R<String> {
    // GeometricShapeType names are the OOXML preset names (`rectangle` → `rect`, `ellipse`…).
    let p = match kind {
        "rectangle" => "rect",
        "roundRectangle" => "roundRect",
        "lineInverse" => "lineInv",
        "rightTriangle" => "rtTriangle",
        other => other,
    };
    if deckcraft_geom::preset::info(p).is_none() {
        return Err(invalid(format!("`{kind}` isn't a shape type")));
    }
    Ok(p.to_string())
}

/// The ids of `slide`'s top-level shapes.
fn top_ids(s: &Session, slide: SlideId) -> R<Vec<ShapeId>> {
    Ok(slide_of(s, slide)?.shapes.iter().map(|sh| sh.id).collect())
}

/// Runs an insert command on `slide` and answers the new shape's id.
fn add_shape(s: &mut Session, slide: SlideId, cmd: &str, params: Json) -> R<Json> {
    let before = top_ids(s, slide)?;
    on(s, slide, Vec::new(), |s| run(s, cmd, params))?;
    let after = top_ids(s, slide)?;
    let new = after.into_iter().find(|id| !before.contains(id)).ok_or_else(|| OfficeError("GeneralException", "the shape wasn't added".into()))?;
    Ok(json!({"id": new.0.to_string()}))
}

fn call(s: &mut Session, addin: &str, obj: &Obj, method: &str, args: &[Json]) -> R<Json> {
    match (obj, method) {
        (Obj::Slides { selected: false }, "add") => {
            let o = args.first().cloned().unwrap_or(Json::Null);
            let mut params = json!({});
            if let Some(layout) = o.get("layoutId") {
                let id = arg_id(std::slice::from_ref(layout), 0)?;
                let d = doc(s)?;
                let name = d.doc.masters.iter().flat_map(|m| m.layouts.iter()).find(|l| l.id.0 == id).map(|l| l.name.clone());
                params["layout"] = json!(name.ok_or_else(|| not_found(format!("there's no layout {id}")))?);
            }
            if let Some(i) = o.get("index") {
                params["at"] = json!(arg_index(std::slice::from_ref(i), 0)?);
            }
            let before = slide_ids(s, false)?;
            run(s, "slide.new", params)?;
            let after = slide_ids(s, false)?;
            let new =
                after.into_iter().find(|id| !before.contains(id)).ok_or_else(|| OfficeError("GeneralException", "the slide wasn't added".into()))?;
            Ok(json!({"id": new.0.to_string()}))
        }
        (Obj::Slide(id), "delete") => {
            run(s, "slide.delete", json!({"ids": [id.0]}))?;
            Ok(Json::Null)
        }
        (Obj::Slide(id), "setSelectedShapes") => {
            let ids: Vec<u32> =
                args.first().and_then(Json::as_array).into_iter().flatten().map(|v| arg_id(std::slice::from_ref(v), 0)).collect::<R<_>>()?;
            for i in &ids {
                shape_of(s, *id, ShapeId(*i))?;
            }
            let index = doc(s)?.doc.slide_index(*id).ok_or_else(|| not_found("no such slide"))?;
            s.select(|_, sel| {
                sel.target = Target::Slides;
                sel.slide = index;
                sel.shapes = ids.iter().map(|i| ShapeId(*i)).collect();
                sel.text = None;
            })
            .map_err(engine)?;
            Ok(Json::Null)
        }
        (Obj::Presentation, "setSelectedSlides") => {
            let ids: Vec<u32> =
                args.first().and_then(Json::as_array).into_iter().flatten().map(|v| arg_id(std::slice::from_ref(v), 0)).collect::<R<_>>()?;
            for i in &ids {
                slide_of(s, SlideId(*i))?;
            }
            if let Some(first) = ids.first() {
                run(s, "slide.go", json!({"id": first}))?;
            }
            run(s, "slide.selectSlides", json!({"ids": ids}))?;
            Ok(Json::Null)
        }
        (Obj::Shapes { slide, selected: false }, "addTextBox") => {
            let text = arg_str(args, 0).unwrap_or("").to_string();
            let [x, y, w, h] = bounds(args, 1)?;
            add_shape(s, *slide, "insert.textBox", json!({"rect": [x, y, w, h], "text": text}))
        }
        (Obj::Shapes { slide, selected: false }, "addGeometricShape") => {
            let preset = preset_of(arg_str(args, 0).ok_or_else(|| invalid("addGeometricShape needs a shape type"))?)?;
            let [x, y, w, h] = bounds(args, 1)?;
            add_shape(s, *slide, "shape.insert", json!({"preset": preset, "rect": [x, y, w, h]}))
        }
        (Obj::Shapes { slide, selected: false }, "addLine") => {
            let [x, y, w, h] = bounds(args, 1)?;
            add_shape(s, *slide, "shape.insert", json!({"preset": "line", "rect": [x, y, w, h]}))
        }
        (Obj::Shapes { slide, selected: true }, "delete") => {
            let ids = shape_ids(s, *slide, true)?;
            if !ids.is_empty() {
                on(s, *slide, ids.clone(), |s| run(s, "edit.delete", json!({"ids": ids.iter().map(|i| i.0).collect::<Vec<_>>()})))?;
            }
            Ok(Json::Null)
        }
        (Obj::Shape(sl, id), "delete") => {
            on(s, *sl, vec![*id], |s| run(s, "edit.delete", json!({"ids": [id.0]})))?;
            Ok(Json::Null)
        }
        (Obj::Fill(sl, id), "setSolidColor") => {
            let c = color(args.first().unwrap_or(&Json::Null))?;
            on(s, *sl, vec![*id], |s| run(s, "shape.fill", json!({"color": c, "ids": [id.0]})))?;
            Ok(Json::Null)
        }
        (Obj::Fill(sl, id), "clear") => {
            on(s, *sl, vec![*id], |s| run(s, "shape.fill", json!({"none": true, "ids": [id.0]})))?;
            Ok(Json::Null)
        }
        (Obj::Tags(owner), "add") => {
            let key = tag_key(args.first())?;
            let value = tag_value(args.get(1).unwrap_or(&Json::Null))?;
            edit_tags(s, *owner, |tags| {
                let full = tags.len() >= MAX_TAGS;
                match tags.iter_mut().find(|(k, _)| *k == key) {
                    Some(t) => t.1 = value,
                    None if full => return Err(invalid(format!("at most {MAX_TAGS} tags"))),
                    None => tags.push((key, value)),
                }
                Ok(())
            })?;
            Ok(Json::Null)
        }
        (Obj::Tags(owner), "delete") => {
            let key = tag_key(args.first())?;
            edit_tags(s, *owner, |tags| {
                tags.retain(|(k, _)| *k != key);
                Ok(())
            })?;
            Ok(Json::Null)
        }
        (Obj::Presentation, "insertSlidesFromBase64") => insert_slides(s, args),
        (Obj::Document, "getSelectedData") if arg_str(args, 0).is_some_and(|k| k.eq_ignore_ascii_case("slideRange")) => {
            // `{slides: [{id, title, index}]}`: ids as numbers, indexes from 1, as Office answers.
            let d = doc(s)?;
            let ids = slide_ids(s, true)?;
            let slides: Vec<Json> = ids
                .iter()
                .filter_map(|id| {
                    let i = d.doc.slide_index(*id)?;
                    let sl = d.doc.slide(*id)?;
                    Some(json!({"id": id.0, "title": slide_title(sl), "index": i + 1}))
                })
                .collect();
            Ok(json!({"slides": slides}))
        }
        (Obj::Document, "getSelectedData") if arg_str(args, 0).is_some_and(|k| !k.eq_ignore_ascii_case("text")) => Err(OfficeError(
            "NotSupported",
            format!("DeckCraft answers the selection as text or slideRange, not {}", arg_str(args, 0).unwrap_or_default()),
        )),
        (Obj::Document, "setSelectedData")
            if args.get(1).and_then(|o| o.get("coercionType")).and_then(Json::as_str).is_some_and(|k| k.eq_ignore_ascii_case("image")) =>
        {
            insert_image(s, args)
        }
        (Obj::Document, "goToById") => go_to_by_id(s, args),
        (Obj::Document, "getSelectedData") => {
            let d = doc(s)?;
            let text = d
                .selection
                .text
                .as_ref()
                .and_then(|t| {
                    let sh = d.shape(t.shape)?;
                    let ((p0, c0), (p1, c1)) = t.ordered();
                    Some(text_between(sh.text.as_ref()?, (p0, c0), (p1, c1)))
                })
                .or_else(|| {
                    // Shapes selected: their text, one shape per line.
                    let texts: Vec<String> = d.selected_shapes().iter().filter_map(|sh| sh.text.as_ref().map(|t| t.text())).collect();
                    (!texts.is_empty()).then(|| texts.join("\n"))
                })
                .unwrap_or_default();
            Ok(json!(text))
        }
        (Obj::Document, "setSelectedData") => {
            let text = match args.first() {
                Some(Json::String(t)) => t.clone(),
                Some(Json::Array(rows)) => rows
                    .iter()
                    .map(|r| {
                        r.as_array()
                            .map(|c| c.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect::<Vec<_>>().join("\t"))
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            if doc(s)?.selection.text.is_some() {
                run(s, "text.insert", json!({"text": text}))?;
            } else {
                // Nothing being typed into: a new text box, as PowerPoint does.
                run(s, "insert.textBox", json!({"text": text}))?;
            }
            Ok(Json::Null)
        }
        (Obj::Settings, "save") => {
            let all = args.first().cloned().filter(Json::is_object).ok_or_else(|| invalid("settings are an object"))?;
            if all.to_string().len() > 2 << 20 {
                return Err(invalid("settings are limited to 2 MB"));
            }
            let uid = s.active().map(|d| d.uid).unwrap_or(0);
            let mut store = match settings_store().lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            store.insert((uid, addin.to_string()), all);
            Ok(Json::Null)
        }
        (o, m) => Err(OfficeError("NotImplemented", format!("{}.{m}() isn't supported yet", name(o)))),
    }
}

/// The slide id an add-in gives: `"267"`, `267`, or PowerPoint's `"267#"` / `"267#1234"` forms.
fn slide_id_arg(v: &Json) -> R<SlideId> {
    let id = match v {
        Json::String(t) => t.trim().split('#').next().and_then(|n| n.trim().parse::<u32>().ok()),
        other => other.as_u64().and_then(|n| u32::try_from(n).ok()),
    };
    id.map(SlideId).ok_or_else(|| invalid(format!("`{}` isn't a slide id", v.to_string().chars().take(40).collect::<String>())))
}

/// Most bytes a presentation or picture an add-in gives as base64 may have.
const MAX_BASE64: usize = 256 << 20;

/// `presentation.insertSlidesFromBase64(base64, {formatting, sourceSlideIds, targetSlideId})`:
/// Reuse Slides (`slide.reuse`) with the source slides picked by id, after the target slide (by
/// default after the current one).
fn insert_slides(s: &mut Session, args: &[Json]) -> R<Json> {
    let data = arg_str(args, 0).ok_or_else(|| invalid("insertSlidesFromBase64 needs the presentation as base64"))?;
    if data.len() > MAX_BASE64 {
        return Err(invalid("the presentation is limited to 256 MB of base64"));
    }
    let o = args.get(1).cloned().unwrap_or(Json::Null);
    let keep = match o.get("formatting").and_then(Json::as_str) {
        None => false,
        Some(f) if f.eq_ignore_ascii_case("KeepSourceFormatting") => true,
        Some(f) if f.eq_ignore_ascii_case("UseDestinationTheme") => false,
        Some(f) => {
            return Err(invalid(format!(
                "formatting is KeepSourceFormatting or UseDestinationTheme, not {}",
                f.chars().take(40).collect::<String>()
            )));
        }
    };
    let mut params = json!({"data": data, "name": "slides.pptx", "keepFormatting": keep});
    if let Some(Json::Array(wanted)) = o.get("sourceSlideIds") {
        let bytes = super::base64_decode(data).ok_or_else(|| invalid("the presentation isn't base64"))?;
        let src = super::file::open_presentation("slides.pptx", &bytes).map_err(|e| invalid(format!("the presentation can't be read: {e}")))?;
        let mut picked = Vec::new();
        for v in wanted.iter().take(MAX_ITEMS) {
            let id = slide_id_arg(v)?;
            let i = src.slide_index(id).ok_or_else(|| not_found(format!("the inserted presentation has no slide {}", id.0)))?;
            picked.push(i);
        }
        params["slides"] = json!(picked);
    }
    if let Some(t) = o.get("targetSlideId").filter(|t| !t.is_null()) {
        let id = slide_id_arg(t)?;
        let i = doc(s)?.doc.slide_index(id).ok_or_else(|| not_found(format!("there's no slide {}", id.0)))?;
        params["at"] = json!(i + 1);
    }
    run(s, "slide.reuse", params)?;
    Ok(Json::Null)
}

/// `setSelectedDataAsync(base64, {coercionType: "image", imageLeft, imageTop, imageWidth,
/// imageHeight})`: the picture on the current slide; a missing size keeps the picture's aspect.
fn insert_image(s: &mut Session, args: &[Json]) -> R<Json> {
    let data = arg_str(args, 0).ok_or_else(|| invalid("an image is given as base64 text"))?;
    if data.len() > MAX_BASE64 {
        return Err(invalid("the image is limited to 256 MB of base64"));
    }
    let o = args.get(1).cloned().unwrap_or(Json::Null);
    let opt = |k: &str| -> R<Option<f64>> {
        match o.get(k) {
            None | Some(Json::Null) => Ok(None),
            Some(v) => v.as_f64().filter(|v| v.is_finite() && v.abs() < 1e6).map(Some).ok_or_else(|| invalid(format!("{k} is a number of points"))),
        }
    };
    let (left, top, width, height) = (opt("imageLeft")?, opt("imageTop")?, opt("imageWidth")?, opt("imageHeight")?);
    if width.is_some_and(|w| w <= 0.0) || height.is_some_and(|h| h <= 0.0) {
        return Err(invalid("imageWidth and imageHeight are more than 0"));
    }
    let slide = current_slide(s)?;
    let out = on(s, slide, Vec::new(), |s| run(s, "insert.picture", json!({"data": data, "name": "Picture.png"})))?;
    let id = out.get("id").and_then(Json::as_u64).and_then(|i| u32::try_from(i).ok()).map(ShapeId);
    if let Some(id) = id
        && (left.is_some() || top.is_some() || width.is_some() || height.is_some())
    {
        let x = load(s, "", &Obj::Shape(slide, id), &["left".into(), "top".into(), "width".into(), "height".into()])?;
        let g = |k: &str| x.get(k).and_then(Json::as_f64).unwrap_or(0.0);
        let (w0, h0) = (g("width").max(1e-6), g("height").max(1e-6));
        let (w, h) = match (width, height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, h0 * w / w0),
            (None, Some(h)) => (w0 * h / h0, h),
            (None, None) => (w0, h0),
        };
        let params = json!({"x": left.unwrap_or(g("left")), "y": top.unwrap_or(g("top")), "w": w, "h": h, "ids": [id.0]});
        on(s, slide, vec![id], |s| run(s, "shape.setBounds", params))?;
    }
    Ok(Json::Null)
}

/// `goToByIdAsync(id, goToType)`: `slide` (a slide id) or `index` (`first`, `last`, `next`,
/// `previous`, or a slide number from 1).
fn go_to_by_id(s: &mut Session, args: &[Json]) -> R<Json> {
    let id = args.first().cloned().unwrap_or(Json::Null);
    let kind = arg_str(args, 1).unwrap_or("slide");
    let d = doc(s)?;
    let n = d.doc.slides.len();
    let current = d.selection.slide;
    let index = if kind.eq_ignore_ascii_case("slide") {
        let sid = slide_id_arg(&id)?;
        d.doc.slide_index(sid).ok_or_else(|| not_found(format!("there's no slide {}", sid.0)))?
    } else if kind.eq_ignore_ascii_case("index") {
        match &id {
            Json::String(w) if w.eq_ignore_ascii_case("first") => 0,
            Json::String(w) if w.eq_ignore_ascii_case("last") => n.saturating_sub(1),
            Json::String(w) if w.eq_ignore_ascii_case("next") => (current + 1).min(n.saturating_sub(1)),
            Json::String(w) if w.eq_ignore_ascii_case("previous") => current.saturating_sub(1),
            other => {
                let k = other.as_u64().or_else(|| other.as_str().and_then(|t| t.trim().parse().ok())).unwrap_or(0);
                usize::try_from(k)
                    .ok()
                    .filter(|k| (1..=n).contains(k))
                    .map(|k| k - 1)
                    .ok_or_else(|| not_found(format!("there's no slide number {other}")))?
            }
        }
    } else {
        return Err(OfficeError(
            "NotSupported",
            format!("DeckCraft goes to slides (by id or index), not to a {}", kind.chars().take(40).collect::<String>()),
        ));
    };
    if n == 0 {
        return Err(not_found("the presentation has no slides"));
    }
    run(s, "slide.go", json!({"index": index}))?;
    Ok(Json::Null)
}

#[cfg(test)]
mod tests;
