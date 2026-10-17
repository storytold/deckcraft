//! What a `RequestContext` keeps between syncs, independent of JavaScript: the queued actions,
//! where each load's result goes, the objects added in the batch, and the properties known so
//! far, keyed by object path. Proxies are a context and a path, so `shape.textFrame.textRange`
//! reached twice is the same object.

use std::collections::HashMap;

use serde_json::{Map, Value as Json, json};

/// A path from a root (`presentation`, `document`, `settings`) to an object: steps as JSON
/// (`{"p": "slides"}`, `{"m": "getItem", "a": ["257"]}`).
pub type Path = Vec<Json>;

/// A property step.
pub fn prop(name: &str) -> Json {
    json!({"p": name})
}

/// A method step.
pub fn method(name: &str, args: Vec<Json>) -> Json {
    json!({"m": name, "a": args})
}

/// The step from a collection to the object a call of the same context added
/// ([`Ctx::call_adding`]): `getItem` of a reference the context replaces on the wire, by the
/// object's id once known, or by `{"result": i}` (the id answered by action `i` of the same sync)
/// while the add is still queued.
pub fn added(reference: usize) -> Json {
    json!({"m": "getItem", "a": [{"ref": reference}]})
}

/// `path` with `step` after it.
pub fn child(path: &[Json], step: Json) -> Path {
    let mut p = path.to_vec();
    p.push(step);
    p
}

fn key(path: &[Json]) -> String {
    serde_json::to_string(path).unwrap_or_default()
}

/// An id as the host answers it (`"257"` or `257`), as a string.
pub fn id_string(v: &Json) -> Option<String> {
    match v {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// An Office.js error (`OfficeExtension.Error`): its code, message and which action failed.
#[derive(Clone, Debug, PartialEq)]
pub struct OfficeError {
    pub code: String,
    pub message: String,
    pub action: Option<u64>,
}

impl OfficeError {
    pub fn new(code: &str, message: impl Into<String>) -> OfficeError {
        OfficeError { code: code.into(), message: message.into(), action: None }
    }
    pub fn not_loaded(prop: &str) -> OfficeError {
        OfficeError::new(
            "PropertyNotLoaded",
            format!(
                "The property '{prop}' is not available. Before reading the property's value, call the load method on the containing object and call \"context.sync()\" on the associated request context."
            ),
        )
    }
    pub fn not_implemented(what: &str) -> OfficeError {
        OfficeError::new("NotImplemented", format!("{what} isn't supported by DeckCraft yet"))
    }
}

/// An object added by a queued call: the collection it was added to, and the action that adds
/// it (while queued) or its id (once a sync answered).
struct Added {
    at: Path,
    action: Option<usize>,
    id: Option<String>,
}

/// What a sync waits for: loads (the action's index, the object's path) and adds (the
/// reference, the action's index).
pub struct Pending {
    loads: Vec<(usize, Path)>,
    adds: Vec<(usize, usize)>,
}

#[derive(Default)]
pub struct Ctx {
    queue: Vec<Json>,
    loads: Vec<(usize, Path)>,
    adds: Vec<Added>,
    cache: HashMap<String, Map<String, Json>>,
}

impl Ctx {
    pub fn new() -> Ctx {
        Ctx::default()
    }

    /// Whether actions are waiting for a sync.
    pub fn has_queued(&self) -> bool {
        !self.queue.is_empty()
    }

    /// `path` as the host reads it: references to added objects replaced by their ids, by the
    /// queued action that adds them, or (an add whose sync failed) by `""`, which no object has.
    pub fn wire(&self, path: &[Json]) -> Path {
        path.iter()
            .map(|step| {
                let Some(args) = step.get("a").and_then(Json::as_array) else { return step.clone() };
                if !args.iter().any(|a| a.get("ref").is_some()) {
                    return step.clone();
                }
                let args: Vec<Json> = args
                    .iter()
                    .map(|a| match a.get("ref").and_then(Json::as_u64).and_then(|r| usize::try_from(r).ok()) {
                        Some(r) => match self.adds.get(r) {
                            Some(Added { id: Some(id), .. }) => json!(id),
                            Some(Added { action: Some(i), .. }) => json!({"result": i}),
                            _ => json!(""),
                        },
                        None => a.clone(),
                    })
                    .collect();
                let mut step = step.clone();
                step["a"] = Json::Array(args);
                step
            })
            .collect()
    }

    fn push(&mut self, mut action: Json) -> usize {
        if let Some(path) = action.get("path").and_then(Json::as_array) {
            action["path"] = Json::Array(self.wire(path));
        }
        self.queue.push(action);
        self.queue.len() - 1
    }

    fn push_load(&mut self, path: &[Json], props: Vec<String>) {
        let i = self.push(json!({"op": "load", "path": path, "props": props}));
        self.loads.push((i, path.to_vec()));
    }

    /// Queues loading `props` of the object at `path`. A property path (`textFrame/textRange/text`)
    /// loads that property of the object it leads to; `items` and `items/name` load a
    /// collection's items (with `name` of each); no properties loads the host's defaults.
    pub fn load(&mut self, path: &[Json], props: &[String]) -> Result<(), OfficeError> {
        let mut own = Vec::new();
        let mut items: Option<Vec<String>> = None;
        let mut nested: Vec<(Path, Vec<String>)> = Vec::new();
        for p in props.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            let parts: Vec<&str> = p.split('/').map(str::trim).collect();
            if parts.first() == Some(&"items") {
                let list = items.get_or_insert_with(Vec::new);
                match parts.as_slice() {
                    [_] => {}
                    [_, sub] => {
                        if !list.iter().any(|q| q == sub) {
                            list.push(sub.to_string());
                        }
                    }
                    _ => return Err(OfficeError::not_implemented(&format!("loading '{p}' (a property of the items' objects)"))),
                }
                continue;
            }
            match parts.split_last() {
                Some((last, [])) => own.push(last.to_string()),
                Some((last, steps)) => {
                    let mut sub = path.to_vec();
                    sub.extend(steps.iter().map(|s| prop(s)));
                    match nested.iter_mut().find(|(q, _)| *q == sub) {
                        Some((_, list)) => list.push(last.to_string()),
                        None => nested.push((sub, vec![last.to_string()])),
                    }
                }
                None => {}
            }
        }
        let had_items = items.is_some();
        if let Some(list) = items {
            let mut props = vec!["items".to_string()];
            props.extend(list);
            self.push_load(path, props);
        }
        if !own.is_empty() || (nested.is_empty() && !had_items) {
            self.push_load(path, own);
        }
        for (sub, list) in nested {
            self.push_load(&sub, list);
        }
        Ok(())
    }

    /// Queues setting a property; the new value is known at once.
    pub fn set(&mut self, path: &[Json], prop: &str, value: Json) {
        self.push(json!({"op": "set", "path": path, "prop": prop, "value": value.clone()}));
        self.cache.entry(key(path)).or_default().insert(prop.to_string(), value);
    }

    /// Queues a method called for its effect.
    pub fn call(&mut self, path: &[Json], method: &str, args: Vec<Json>) {
        self.push(json!({"op": "call", "path": path, "method": method, "args": args}));
    }

    /// Queues a method that adds an object to the collection at `path` and answers its id; the
    /// returned reference names the object ([`added`]) before and after the sync.
    pub fn call_adding(&mut self, path: &[Json], method: &str, args: Vec<Json>) -> usize {
        let i = self.push(json!({"op": "call", "path": path, "method": method, "args": args}));
        self.adds.push(Added { at: path.to_vec(), action: Some(i), id: None });
        self.adds.len() - 1
    }

    /// A property known so far (loaded, or set since).
    pub fn get(&self, path: &[Json], prop: &str) -> Option<&Json> {
        self.cache.get(&key(path)).and_then(|m| m.get(prop))
    }

    /// Every property known so far of the object at `path`.
    pub fn object(&self, path: &[Json]) -> Option<&Map<String, Json>> {
        self.cache.get(&key(path))
    }

    /// The queued actions, taken for one sync, and what waits for its answer.
    pub fn take(&mut self) -> (Vec<Json>, Pending) {
        let mut adds = Vec::new();
        for (r, a) in self.adds.iter_mut().enumerate() {
            if let Some(i) = a.action.take() {
                adds.push((r, i));
            }
        }
        (std::mem::take(&mut self.queue), Pending { loads: std::mem::take(&mut self.loads), adds })
    }

    /// Reads a sync's answer: `{"results": [...]}` fills the loaded objects and the ids of the
    /// added ones; `{"error": …}` is the error the sync rejects with.
    pub fn apply(&mut self, pending: Pending, reply: &Json) -> Result<Json, OfficeError> {
        let results = results(reply)?;
        for (r, i) in pending.adds {
            let Some(id) = results.get(i).and_then(|v| v.get("id")).and_then(id_string) else { continue };
            if let Some(a) = self.adds.get_mut(r) {
                a.id = Some(id.clone());
                let at = child(&a.at, added(r));
                self.cache.entry(key(&at)).or_default().insert("id".into(), json!(id));
            }
        }
        for (i, path) in pending.loads {
            let Some(Json::Object(props)) = results.get(i) else { continue };
            // A collection's items become objects of their own: `shapes.items[0].name`.
            if let Some(Json::Array(items)) = props.get("items") {
                for item in items {
                    if let Some(id) = item.get("id").or_else(|| item.get("key")).and_then(id_string)
                        && let Json::Object(fields) = item
                    {
                        let at = child(&path, method("getItem", vec![json!(id)]));
                        self.cache.entry(key(&at)).or_default().extend(fields.clone());
                    }
                }
            }
            self.cache.entry(key(&path)).or_default().extend(props.clone());
        }
        Ok(Json::Array(results.to_vec()))
    }
}

/// A sync's answer: the results, one per action, or the error it carries.
pub fn results(reply: &Json) -> Result<&[Json], OfficeError> {
    if let Some(e) = reply.get("error") {
        return Err(OfficeError {
            code: e.get("code").and_then(Json::as_str).unwrap_or("GeneralException").to_string(),
            message: e.get("message").and_then(Json::as_str).unwrap_or("The request failed").to_string(),
            action: e.get("action").and_then(Json::as_u64),
        });
    }
    reply
        .get("results")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| OfficeError::new("GeneralException", "the host's answer has no results"))
}

/// The property names a `load()` argument names: `"a, b"`, `["a", "b"]`, `{select: "a,b"}` or
/// `{a: true, b: true}` (already turned into JSON).
pub fn load_props(arg: &Json) -> Vec<String> {
    match arg {
        Json::String(s) => s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
        Json::Array(list) => list.iter().filter_map(Json::as_str).map(str::to_string).collect(),
        Json::Object(o) => match o.get("select") {
            Some(sel) => load_props(sel),
            None => o.iter().filter(|(_, v)| v.as_bool() == Some(true)).map(|(k, _)| k.clone()).collect(),
        },
        _ => Vec::new(),
    }
}

fn first_char(s: &str, upper: bool) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if upper => c.to_uppercase().chain(chars).collect(),
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A PowerPoint enum value as the host takes it: the API's `"GeometricShape"`, `"Rectangle"`,
/// `"AutoSizeNone"` are `geometricShape`, `rectangle`, `autoSizeNone` on the wire.
pub fn enum_to_wire(v: &Json) -> Json {
    match v {
        Json::String(s) => json!(first_char(s, false)),
        other => other.clone(),
    }
}

/// A host's enum value (`geometricShape`) as the API answers it (`"GeometricShape"`).
pub fn enum_from_wire(v: &Json) -> Json {
    match v {
        Json::String(s) => json!(first_char(s, true)),
        other => other.clone(),
    }
}

/// The requirement sets reported as supported, with the highest version. The plugin host checks
/// Office add-ins' `<Requirements>` against the same table (`deckcraft_plugin::office::API_SETS`;
/// a test keeps them equal).
pub const REQUIREMENT_SETS: &[(&str, (u32, u32))] = &[
    ("PowerPointApi", (1, 4)),
    ("Settings", (1, 1)),
    ("Selection", (1, 1)),
    ("TextCoercion", (1, 1)),
    ("ImageCoercion", (1, 1)),
    ("DocumentEvents", (1, 1)),
    ("AddinCommands", (1, 1)),
];

/// The Common API methods this library implements (`<Requirements><Methods>`), as the plugin
/// host checks them (`deckcraft_plugin::office::API_METHODS`).
pub const SUPPORTED_METHODS: &[&str] = &[
    "Document.getSelectedDataAsync",
    "Document.setSelectedDataAsync",
    "Document.goToByIdAsync",
    "Settings.get",
    "Settings.set",
    "Settings.remove",
    "Settings.saveAsync",
];

/// `Office.context.requirements.isSetSupported(name, minVersion)`.
pub fn is_set_supported(name: &str, min: Option<&str>) -> bool {
    let Some((_, have)) = REQUIREMENT_SETS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) else { return false };
    let want = min.map(version).unwrap_or((1, 1));
    want <= *have
}

/// A function command the host runs (`__craftOfficeRunFunction({"call", "name", "control"})`):
/// the host's number for the call, the function's name and the manifest control that ran it
/// (`event.source.id`).
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionCall {
    pub call: Json,
    pub name: String,
    pub control: String,
}

impl FunctionCall {
    /// Reads the host's `{"call", "name", "control"}`; `None` without a usable name.
    pub fn parse(text: &str) -> Option<FunctionCall> {
        let v: Json = serde_json::from_str(text).ok()?;
        let name = v.get("name").and_then(Json::as_str).map(str::trim).filter(|n| !n.is_empty() && n.len() <= 256)?.to_string();
        let control = v.get("control").and_then(Json::as_str).unwrap_or_default().chars().take(256).collect();
        Some(FunctionCall { call: v.get("call").cloned().unwrap_or(Json::Null), name, control })
    }

    /// The event telling the host the call is over (`event.completed()`), or what went wrong.
    pub fn done(&self, error: Option<&str>) -> Json {
        let mut m = json!({"event": "functionDone", "call": self.call});
        if let Some(e) = error {
            m["error"] = json!(e.chars().take(500).collect::<String>());
        }
        m
    }
}

/// `set({...})`'s properties as (navigation steps, property, value): an object under a
/// navigation property (`is_nav`) is followed (`{font: {bold: true}}` → `(["font"], "bold",
/// true)`); anything else is a value. At most 16 levels and 1 000 properties.
pub fn flatten_set(value: &Json, is_nav: impl Fn(&str) -> bool) -> Result<Vec<(Vec<String>, String, Json)>, OfficeError> {
    fn walk(v: &Json, at: &mut Vec<String>, is_nav: &dyn Fn(&str) -> bool, out: &mut Vec<(Vec<String>, String, Json)>) -> Result<(), OfficeError> {
        let Json::Object(o) = v else { return Err(OfficeError::new("InvalidArgument", "set() takes an object of properties")) };
        if at.len() > 16 || out.len() > 1000 {
            return Err(OfficeError::new("InvalidArgument", "set() takes at most 16 levels and 1000 properties"));
        }
        for (k, v) in o {
            if v.is_object() && is_nav(k) {
                at.push(k.clone());
                walk(v, at, is_nav, out)?;
                at.pop();
            } else {
                out.push((at.clone(), k.clone(), v.clone()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(value, &mut Vec::new(), &is_nav, &mut out)?;
    Ok(out)
}

fn version(v: &str) -> (u32, u32) {
    let mut parts = v.trim().split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(1), parts.next().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plugin host blocks Office add-ins that need more than this library reports: both read
    /// the same table.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_plugin_host_checks_the_same_requirement_sets() {
        let host: Vec<(String, (u64, u64))> = deckcraft_plugin::office::API_SETS.iter().map(|(n, v)| (n.to_string(), *v)).collect();
        let here: Vec<(String, (u64, u64))> = REQUIREMENT_SETS.iter().map(|(n, (a, b))| (n.to_string(), (u64::from(*a), u64::from(*b)))).collect();
        assert_eq!(host, here);
        assert_eq!(deckcraft_plugin::office::API_METHODS, SUPPORTED_METHODS);
    }

    fn shapes() -> Path {
        vec![prop("presentation"), prop("slides"), method("getItemAt", vec![json!(0)]), prop("shapes")]
    }

    #[test]
    fn loads_fill_the_objects_their_paths_name() {
        let mut c = Ctx::new();
        let shape = child(&shapes(), method("getItem", vec![json!("260")]));
        assert!(c.get(&shape, "name").is_none());
        c.load(&shape, &["name".into(), "textFrame/textRange/text".into(), "left".into()]).unwrap();
        let (actions, pending) = c.take();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], json!({"op": "load", "path": shape, "props": ["name", "left"]}));
        assert_eq!(actions[1]["path"][6], json!({"p": "textRange"}));
        assert!(!c.has_queued());
        c.apply(pending, &json!({"results": [{"name": "TextBox 1", "left": 10}, {"text": "Hi"}]})).unwrap();
        assert_eq!(c.get(&shape, "name"), Some(&json!("TextBox 1")));
        let range = child(&child(&shape, prop("textFrame")), prop("textRange"));
        assert_eq!(c.get(&range, "text"), Some(&json!("Hi")));
    }

    #[test]
    fn empty_load_asks_for_the_defaults_once() {
        let mut c = Ctx::new();
        let s = shapes();
        c.load(&s, &[]).unwrap();
        assert_eq!(c.take().0, vec![json!({"op": "load", "path": s, "props": []})]);
    }

    #[test]
    fn sets_are_known_at_once_and_errors_carry_their_code() {
        let mut c = Ctx::new();
        let font = vec![prop("presentation"), prop("x"), prop("font")];
        c.set(&font, "bold", json!(true));
        assert_eq!(c.get(&font, "bold"), Some(&json!(true)));
        c.call(&font, "delete", vec![]);
        let (actions, pending) = c.take();
        assert_eq!(actions[1]["method"], "delete");
        let e = c.apply(pending, &json!({"error": {"code": "ItemNotFound", "message": "no slide", "action": 1}})).unwrap_err();
        assert_eq!((e.code.as_str(), e.action), ("ItemNotFound", Some(1)));
    }

    #[test]
    fn collection_items_become_objects_by_id() {
        let mut c = Ctx::new();
        let s = shapes();
        c.load(&s, &["items/name".into(), "items/left".into(), "items".into()]).unwrap();
        c.load(&s, &["count".into(), "items".into()]).unwrap();
        let (actions, pending) = c.take();
        assert_eq!(actions[0], json!({"op": "load", "path": s, "props": ["items", "name", "left"]}));
        assert_eq!(actions[1]["props"], json!(["items"]));
        assert_eq!(actions[2]["props"], json!(["count"]));
        let r = c.apply(pending, &json!({"results": [{"items": [{"id": 260, "name": "A", "left": 0}]}, {"items": [{"id": "260"}]}, {"count": 1}]}));
        assert!(r.is_ok());
        assert_eq!(c.get(&child(&s, method("getItem", vec![json!("260")])), "name"), Some(&json!("A")));
        assert_eq!(c.get(&s, "count"), Some(&json!(1)));
        assert!(c.load(&s, &["items/textFrame/textRange/text".into()]).is_err());
    }

    #[test]
    fn added_objects_are_named_by_result_then_by_id() {
        let mut c = Ctx::new();
        let s = shapes();
        c.set(&s, "unrelated", json!(1));
        let r = c.call_adding(&s, "addTextBox", vec![json!("Hi"), json!({"left": 10})]);
        let shape = child(&s, added(r));
        c.set(&child(&shape, prop("textFrame")), "wordWrap", json!(true));
        c.load(&shape, &["name".into()]).unwrap();
        let (actions, pending) = c.take();
        assert_eq!(actions[2]["path"][4], json!({"m": "getItem", "a": [{"result": 1}]}));
        assert_eq!(actions[3]["path"][4], json!({"m": "getItem", "a": [{"result": 1}]}));
        c.apply(pending, &json!({"results": [null, {"id": "261"}, null, {"name": "TextBox 2"}]})).unwrap();
        assert_eq!(c.get(&shape, "id"), Some(&json!("261")));
        assert_eq!(c.get(&shape, "name"), Some(&json!("TextBox 2")));
        c.call(&shape, "delete", vec![]);
        assert_eq!(c.take().0[0]["path"][4], json!({"m": "getItem", "a": ["261"]}));
        // An add whose sync failed names no object.
        let r2 = c.call_adding(&s, "addLine", vec![json!("straight")]);
        let (_, pending) = c.take();
        assert!(c.apply(pending, &json!({"error": {"code": "InvalidArgument"}})).is_err());
        c.call(&child(&s, added(r2)), "delete", vec![]);
        assert_eq!(c.take().0[0]["path"][4], json!({"m": "getItem", "a": [""]}));
    }

    #[test]
    fn enums_and_load_arguments_and_requirement_sets() {
        assert_eq!(enum_to_wire(&json!("GeometricShape")), json!("geometricShape"));
        assert_eq!(enum_to_wire(&json!("rightArrow")), json!("rightArrow"));
        assert_eq!(enum_from_wire(&json!("autoSizeNone")), json!("AutoSizeNone"));
        assert_eq!(enum_from_wire(&json!(null)), json!(null));
        assert_eq!(enum_to_wire(&json!("")), json!(""));
        assert_eq!(load_props(&json!("text, start")), ["text", "start"]);
        assert_eq!(load_props(&json!(["text"])), ["text"]);
        assert_eq!(load_props(&json!({"select": "a,b"})), ["a", "b"]);
        assert_eq!(load_props(&json!({"name": true, "id": false})), ["name"]);
        assert!(is_set_supported("PowerPointApi", Some("1.4")) && is_set_supported("powerpointapi", Some("1.1")));
        assert!(is_set_supported("PowerPointApi", None) && is_set_supported("TextCoercion", Some("1.1")));
        assert!(!is_set_supported("PowerPointApi", Some("1.5")) && !is_set_supported("PowerPointApi", Some("1.10")));
        assert!(
            is_set_supported("ImageCoercion", None)
                && is_set_supported("DocumentEvents", Some("1.1"))
                && is_set_supported("AddinCommands", Some("1.1"))
        );
        assert!(!is_set_supported("AddinCommands", Some("1.3")) && !is_set_supported("DialogApi", None));
        assert!(!is_set_supported("ExcelApi", Some("1.1")) && !is_set_supported("MatrixCoercion", None));
        assert_eq!(id_string(&json!(257)), Some("257".into()));
    }

    #[test]
    fn function_calls_are_read_and_answered() {
        let c = FunctionCall::parse(r#"{"call": 3, "name": "writeValue", "control": "Contoso.FunctionButton"}"#).unwrap();
        assert_eq!(c, FunctionCall { call: json!(3), name: "writeValue".into(), control: "Contoso.FunctionButton".into() });
        assert_eq!(c.done(None), json!({"event": "functionDone", "call": 3}));
        assert_eq!(c.done(Some("no")), json!({"event": "functionDone", "call": 3, "error": "no"}));
        let bare = FunctionCall::parse(r#"{"name": " toggle "}"#).unwrap();
        assert_eq!((bare.name.as_str(), bare.control.as_str(), &bare.call), ("toggle", "", &Json::Null));
        for bad in ["", "toggle", r#"{"control": "x"}"#, r#"{"name": ""}"#, &format!(r#"{{"name": "{}"}}"#, "f".repeat(300))] {
            assert!(FunctionCall::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn set_follows_navigation_properties() {
        let nav = |k: &str| matches!(k, "font" | "fill" | "textFrame" | "textRange");
        let mut got = flatten_set(
            &json!({"name": "A", "fill": {"foregroundColor": "red"}, "textFrame": {"textRange": {"font": {"bold": true}}}, "left": 5}),
            nav,
        )
        .unwrap();
        got.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            got,
            vec![
                (vec!["textFrame".to_string(), "textRange".to_string(), "font".to_string()], "bold".to_string(), json!(true)),
                (vec!["fill".to_string()], "foregroundColor".to_string(), json!("red")),
                (vec![], "left".to_string(), json!(5)),
                (vec![], "name".to_string(), json!("A")),
            ]
        );
        // An object under a property that isn't a navigation property is a value.
        assert_eq!(flatten_set(&json!({"tag": {"a": 1}}), nav).unwrap(), vec![(vec![], "tag".to_string(), json!({"a": 1}))]);
        assert!(flatten_set(&json!(5), nav).is_err());
        let mut deep = json!({"bold": true});
        for _ in 0..40 {
            deep = json!({"font": deep});
        }
        assert!(flatten_set(&deep, nav).is_err());
    }
}
