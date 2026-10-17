//! The JavaScript side: the `Office`, `PowerPoint` and `OfficeExtension` globals, the proxy
//! classes, and the transport to the host.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use js_sys::{Array, Function, Object, Promise, Reflect};
use serde_json::{Map, Value as Json, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise, spawn_local};

use crate::core::{
    Ctx, FunctionCall, OfficeError, Path, added, child, enum_from_wire, enum_to_wire, flatten_set, id_string, is_set_supported, load_props, method,
    prop,
};

/// A synchronous transport (tests answer in-process).
type SyncTransport = Rc<dyn Fn(String) -> String>;

thread_local! {
    static TRANSPORT: RefCell<Option<SyncTransport>> = const { RefCell::new(None) };
    static WAITING: RefCell<HashMap<u64, Function>> = RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    static SETTINGS: RefCell<Map<String, Json>> = RefCell::new(Map::new());
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    static READY: RefCell<Option<Promise>> = const { RefCell::new(None) };
    /// Whether `READY` has resolved (a late `Office.initialize` is called at once).
    static IS_READY: Cell<bool> = const { Cell::new(false) };
    /// `Office.initialize`, as the page set it.
    static INITIALIZE: RefCell<Option<Function>> = const { RefCell::new(None) };
    /// `Office.actions.associate(name, fn)`: the add-in's function commands.
    static ACTIONS: RefCell<HashMap<String, Function>> = RefCell::new(HashMap::new());
    /// `Office.context.document.addHandlerAsync(DocumentSelectionChanged, fn)`.
    static SELECTION_HANDLERS: RefCell<Vec<Function>> = const { RefCell::new(Vec::new()) };
}

// ---------------------------------------------------------------- values and errors

pub(crate) fn to_js(v: &Json) -> JsValue {
    match v {
        Json::Null => JsValue::NULL,
        _ => js_sys::JSON::parse(&v.to_string()).unwrap_or(JsValue::NULL),
    }
}

pub(crate) fn from_js(v: &JsValue) -> Json {
    if v.is_undefined() || v.is_null() {
        return Json::Null;
    }
    js_sys::JSON::stringify(v).ok().and_then(|s| s.as_string()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Json::Null)
}

/// An `OfficeExtension.Error`: an `Error` named `RichApi.Error` with `code` and `debugInfo`.
fn js_error(e: &OfficeError) -> JsValue {
    let err = js_sys::Error::new(&e.message);
    err.set_name("RichApi.Error");
    let _ = Reflect::set(&err, &"code".into(), &e.code.as_str().into());
    let debug = to_js(&json!({"code": e.code, "message": e.message, "errorLocation": e.action}));
    let _ = Reflect::set(&err, &"debugInfo".into(), &debug);
    err.into()
}

fn not_implemented(what: &str) -> JsValue {
    js_error(&OfficeError::not_implemented(what))
}

// ---------------------------------------------------------------- the transport

/// Sends one batch of actions and waits for the host's answer (`{"results"}` or `{"error"}`).
async fn send(actions: Vec<Json>) -> Result<Json, OfficeError> {
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id.wrapping_add(1));
        id
    });
    let request = json!({"id": id, "actions": actions}).to_string();
    if let Some(t) = TRANSPORT.with(|t| t.borrow().clone()) {
        return serde_json::from_str(&t(request)).map_err(|e| OfficeError::new("GeneralException", format!("the host's answer isn't JSON: {e}")));
    }
    let global = js_sys::global();
    let host = Reflect::get(&global, &"__craftOffice".into()).unwrap_or(JsValue::UNDEFINED);
    let post = Reflect::get(&host, &"post".into()).ok().and_then(|f| f.dyn_into::<Function>().ok());
    let Some(post) = post else { return Err(OfficeError::new("GeneralException", "this page isn't running in DeckCraft")) };
    let answer = Promise::new(&mut |resolve, _reject| {
        WAITING.with(|w| w.borrow_mut().insert(id, resolve));
    });
    if post.call1(&host, &request.into()).is_err() {
        WAITING.with(|w| w.borrow_mut().remove(&id));
        return Err(OfficeError::new("GeneralException", "DeckCraft didn't take the request"));
    }
    let reply = JsFuture::from(answer).await.map_err(|_| OfficeError::new("GeneralException", "the request was dropped"))?;
    let text = reply.as_string().unwrap_or_default();
    serde_json::from_str(&text).map_err(|e| OfficeError::new("GeneralException", format!("the host's answer isn't JSON: {e}")))
}

/// `globalThis.__craftOfficeReply(json)`: the host's answer to a request, by its `id`.
fn reply(text: String) {
    let id = serde_json::from_str::<Json>(&text).ok().and_then(|v| v.get("id").and_then(Json::as_u64));
    if let Some(id) = id
        && let Some(resolve) = WAITING.with(|w| w.borrow_mut().remove(&id))
    {
        let _ = resolve.call1(&JsValue::UNDEFINED, &text.into());
    }
}

// ---------------------------------------------------------------- object kinds

/// What the library knows of a class: the properties `load()` with no argument asks for, the
/// properties holding PowerPoint enum values, and (collections) the kind of the items and the
/// property that identifies them.
struct Kind {
    defaults: &'static [&'static str],
    enums: &'static [&'static str],
    item: Option<&'static Kind>,
}

impl Kind {
    const fn object(defaults: &'static [&'static str], enums: &'static [&'static str]) -> Kind {
        Kind { defaults, enums, item: None }
    }
    const fn collection(defaults: &'static [&'static str], item: &'static Kind) -> Kind {
        Kind { defaults, enums: &[], item: Some(item) }
    }
    fn out(&self, name: &str, v: &Json) -> Json {
        if self.enums.contains(&name) { enum_from_wire(v) } else { v.clone() }
    }
}

static PRESENTATION: Kind = Kind::object(&["title"], &[]);
static SLIDE: Kind = Kind::object(&["id", "index"], &[]);
static SLIDES: Kind = Kind::collection(&["items/index"], &SLIDE);
static LAYOUT: Kind = Kind::object(&["id", "name"], &[]);
static LAYOUTS: Kind = Kind::collection(&["items/name"], &LAYOUT);
static MASTER: Kind = Kind::object(&["id", "name"], &[]);
static MASTERS: Kind = Kind::collection(&["items/name"], &MASTER);
static SHAPE: Kind = Kind::object(&["id", "name", "type", "left", "top", "width", "height"], &["type"]);
static SHAPES: Kind = Kind::collection(&["items/name", "items/type", "items/left", "items/top", "items/width", "items/height"], &SHAPE);
static TEXT_FRAME: Kind = Kind::object(
    &["hasText", "wordWrap", "autoSizeSetting", "leftMargin", "rightMargin", "topMargin", "bottomMargin", "verticalAlignment"],
    &["autoSizeSetting", "verticalAlignment"],
);
static TEXT_RANGE: Kind = Kind::object(&["text", "start", "length"], &[]);
static FONT: Kind = Kind::object(&["bold", "italic", "underline", "size", "name", "color"], &["underline"]);
static PARAGRAPH: Kind = Kind::object(&["horizontalAlignment"], &["horizontalAlignment"]);
static BULLETS: Kind = Kind::object(&["visible"], &[]);
static FILL: Kind = Kind::object(&["type", "foregroundColor", "transparency"], &["type"]);
static LINE: Kind = Kind::object(&["color", "weight", "visible", "dashStyle", "style", "transparency"], &["dashStyle", "style"]);
static TAG: Kind = Kind::object(&["key", "value"], &[]);
static TAGS: Kind = Kind::collection(&["items/value"], &TAG);

// ---------------------------------------------------------------- proxies

/// A context, a path and a kind: what every proxy object is.
#[derive(Clone)]
struct P {
    ctx: Rc<RefCell<Ctx>>,
    path: Path,
    kind: &'static Kind,
}

impl P {
    fn to(&self, step: Json, kind: &'static Kind) -> P {
        P { ctx: self.ctx.clone(), path: child(&self.path, step), kind }
    }
    fn get(&self, name: &str) -> Result<JsValue, JsValue> {
        self.ctx.borrow().get(&self.path, name).map(|v| to_js(&self.kind.out(name, v))).ok_or_else(|| js_error(&OfficeError::not_loaded(name)))
    }
    fn set(&self, name: &str, v: &JsValue) {
        let v = from_js(v);
        let v = if self.kind.enums.contains(&name) { enum_to_wire(&v) } else { v };
        self.ctx.borrow_mut().set(&self.path, name, v);
    }
    /// `set({...})`: each property set, navigation properties (`{font: {bold: true}}`) followed.
    fn set_many(&self, v: &JsValue) -> Result<(), JsValue> {
        let v = from_js(v);
        if !v.is_object() {
            return Err(js_error(&OfficeError::new("InvalidArgument", "set() takes an object of properties")));
        }
        for (steps, name, value) in flatten_set(&v, |k| nav(k).is_some()).map_err(|e| js_error(&e))? {
            let mut target = self.clone();
            for step in &steps {
                target = target.to(prop(step), nav(step).unwrap_or(&SHAPE));
            }
            target.set(&name, &to_js(&value));
        }
        Ok(())
    }
    fn load(&self, arg: &JsValue) -> Result<(), JsValue> {
        let props =
            if arg.is_undefined() || arg.is_null() { self.kind.defaults.iter().map(|s| s.to_string()).collect() } else { load_props(&from_js(arg)) };
        self.ctx.borrow_mut().load(&self.path, &props).map_err(|e| js_error(&e))
    }
    fn call(&self, name: &str, args: Vec<Json>) {
        self.ctx.borrow_mut().call(&self.path, name, args);
    }
    /// Queues a call that adds an item to this collection; the proxy of the new item.
    fn add(&self, name: &str, args: Vec<Json>) -> P {
        let r = self.ctx.borrow_mut().call_adding(&self.path, name, args);
        self.to(added(r), self.kind.item.unwrap_or(&SHAPE))
    }
    /// A collection's item by key (`getItem`).
    fn item(&self, key: &str) -> P {
        self.to(method("getItem", vec![json!(key)]), self.kind.item.unwrap_or(&SHAPE))
    }
    fn item_at(&self, index: f64) -> P {
        self.to(method("getItemAt", vec![num(index)]), self.kind.item.unwrap_or(&SHAPE))
    }
    /// `getCount()`: a load of `count`, read from the `ClientResult` after the sync.
    fn count(&self) -> ClientResult {
        let _ = self.ctx.borrow_mut().load(&self.path, &["count".to_string()]);
        ClientResult { p: self.clone(), prop: "count".into() }
    }
    /// The loaded items' identities (`id`, or `key` for tags).
    fn item_keys(&self) -> Result<Vec<String>, JsValue> {
        let items = self.ctx.borrow().get(&self.path, "items").cloned().ok_or_else(|| js_error(&OfficeError::not_loaded("items")))?;
        Ok(items.as_array().into_iter().flatten().filter_map(|i| i.get("id").or_else(|| i.get("key")).and_then(id_string)).collect())
    }
    fn items_as<T: From<P> + Into<JsValue>>(&self) -> Result<Array, JsValue> {
        let out = Array::new();
        for k in self.item_keys()? {
            out.push(&T::from(self.item(&k)).into());
        }
        Ok(out)
    }
    /// `toJSON()`: the properties loaded so far (a collection: `{items: [...]}`).
    fn to_json(&self) -> JsValue {
        let ctx = self.ctx.borrow();
        let own = |p: &P| -> Json {
            let props = ctx.object(&p.path).cloned().unwrap_or_default();
            Json::Object(props.iter().map(|(k, v)| (k.clone(), p.kind.out(k, v))).collect())
        };
        if let Some(item_kind) = self.kind.item {
            let keys: Vec<String> = ctx
                .get(&self.path, "items")
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(|i| i.get("id").or_else(|| i.get("key")).and_then(id_string))
                .collect();
            let items: Vec<Json> = keys
                .iter()
                .map(|k| own(&P { ctx: self.ctx.clone(), path: child(&self.path, method("getItem", vec![json!(k)])), kind: item_kind }))
                .collect();
            return to_js(&json!({"items": items}));
        }
        to_js(&own(self))
    }
}

/// The kind of a navigation property `set({...})` follows.
fn nav(name: &str) -> Option<&'static Kind> {
    Some(match name {
        "textFrame" => &TEXT_FRAME,
        "textRange" => &TEXT_RANGE,
        "font" => &FONT,
        "paragraphFormat" => &PARAGRAPH,
        "bulletFormat" => &BULLETS,
        "fill" => &FILL,
        "lineFormat" => &LINE,
        _ => return None,
    })
}

fn num(v: f64) -> Json {
    if v.fract() == 0.0 && v.is_finite() && v.abs() < 1e15 { json!(v as i64) } else { json!(v) }
}

/// The fields of an options object an add takes (`ShapeAddOptions`, `AddSlideOptions`).
fn options(v: &JsValue, keys: &[&str]) -> Json {
    let all = from_js(v);
    let picked: Map<String, Json> = keys.iter().filter_map(|k| all.get(*k).filter(|v| !v.is_null()).map(|v| (k.to_string(), v.clone()))).collect();
    Json::Object(picked)
}

const SHAPE_OPTIONS: &[&str] = &["left", "top", "width", "height"];

/// A list of ids (`setSelectedSlides(["257", "258"])`).
fn ids(v: &JsValue) -> Json {
    Json::Array(from_js(v).as_array().into_iter().flatten().filter_map(id_string).map(Json::String).collect())
}

/// Each proxy class: a [`P`], `load()` (returns the object, as Office.js does) and `toJSON()`.
macro_rules! class {
    ($t:ident) => {
        #[wasm_bindgen]
        pub struct $t {
            p: P,
        }
        impl From<P> for $t {
            fn from(p: P) -> $t {
                $t { p }
            }
        }
        #[wasm_bindgen]
        impl $t {
            pub fn load(&self, props: JsValue) -> Result<$t, JsValue> {
                self.p.load(&props)?;
                Ok($t { p: self.p.clone() })
            }
            #[wasm_bindgen(js_name = toJSON)]
            pub fn to_json(&self) -> JsValue {
                self.p.to_json()
            }
            /// `set(properties)`: several properties at once (navigation properties too).
            pub fn set(&self, props: JsValue) -> Result<(), JsValue> {
                self.p.set_many(&props)
            }
        }
    };
}

/// A read-only property.
macro_rules! ro {
    ($t:ident, $f:ident, $js:literal) => {
        #[wasm_bindgen]
        impl $t {
            #[wasm_bindgen(getter = $js)]
            pub fn $f(&self) -> Result<JsValue, JsValue> {
                self.p.get($js)
            }
        }
    };
}

/// A property that can be set.
macro_rules! rw {
    ($t:ident, $f:ident, $set:ident, $js:literal) => {
        ro!($t, $f, $js);
        #[wasm_bindgen]
        impl $t {
            #[wasm_bindgen(setter = $js)]
            pub fn $set(&self, v: JsValue) {
                self.p.set($js, &v);
            }
        }
    };
}

/// A collection's `getCount()`, `getItem(key)`, `getItemAt(index)` and `items`.
macro_rules! collection {
    ($t:ident, $item:ident) => {
        class!($t);
        #[wasm_bindgen]
        impl $t {
            #[wasm_bindgen(js_name = getCount)]
            pub fn get_count(&self) -> ClientResult {
                self.p.count()
            }
            #[wasm_bindgen(js_name = getItem)]
            pub fn get_item(&self, key: String) -> $item {
                $item { p: self.p.item(&key) }
            }
            #[wasm_bindgen(js_name = getItemAt)]
            pub fn get_item_at(&self, index: f64) -> $item {
                $item { p: self.p.item_at(index) }
            }
            /// The items, once `load("items")` and a sync have run.
            #[wasm_bindgen(getter)]
            pub fn items(&self) -> Result<Array, JsValue> {
                self.p.items_as::<$item>()
            }
        }
    };
}

/// `getCount()`'s answer: `value` once the sync has run.
#[wasm_bindgen]
pub struct ClientResult {
    p: P,
    prop: String,
}

#[wasm_bindgen]
impl ClientResult {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        self.p.ctx.borrow().get(&self.p.path, &self.prop).map(to_js).ok_or_else(|| {
            js_error(&OfficeError::new(
                "ValueNotLoaded",
                "The value of the result object has not been loaded yet. Before reading the value property, call \"context.sync()\" on the associated request context.",
            ))
        })
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct RequestContext {
    p: P,
}

#[wasm_bindgen]
impl RequestContext {
    #[wasm_bindgen(constructor)]
    pub fn new() -> RequestContext {
        RequestContext { p: P { ctx: Rc::new(RefCell::new(Ctx::new())), path: Vec::new(), kind: &PRESENTATION } }
    }
    #[wasm_bindgen(getter)]
    pub fn presentation(&self) -> Presentation {
        Presentation { p: self.p.to(prop("presentation"), &PRESENTATION) }
    }
    /// Sends the queued actions; resolves once loaded properties are filled.
    pub fn sync(&self) -> Promise {
        let ctx = self.p.ctx.clone();
        future_to_promise(async move { sync(ctx).await.map(|_| JsValue::UNDEFINED) })
    }
    /// `context.trackedObjects.add/remove`: objects are found by id, so there's nothing to track.
    #[wasm_bindgen(getter, js_name = trackedObjects)]
    pub fn tracked_objects(&self) -> JsValue {
        let noop = Closure::<dyn Fn(JsValue)>::new(|_| {}).into_js_value();
        obj(&[("add", noop.clone()), ("remove", noop)]).into()
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        RequestContext::new()
    }
}

async fn sync(ctx: Rc<RefCell<Ctx>>) -> Result<(), JsValue> {
    let (actions, pending) = ctx.borrow_mut().take();
    if actions.is_empty() {
        return Ok(());
    }
    let answer = send(actions).await.map_err(|e| js_error(&e))?;
    ctx.borrow_mut().apply(pending, &answer).map(|_| ()).map_err(|e| js_error(&e))
}

// ---------------------------------------------------------------- presentation and slides

class!(Presentation);
ro!(Presentation, title, "title");
ro!(Presentation, id, "id");

#[wasm_bindgen]
impl Presentation {
    #[wasm_bindgen(getter)]
    pub fn slides(&self) -> SlideCollection {
        SlideCollection { p: self.p.to(prop("slides"), &SLIDES) }
    }
    #[wasm_bindgen(getter = slideMasters)]
    pub fn slide_masters(&self) -> SlideMasterCollection {
        SlideMasterCollection { p: self.p.to(prop("slideMasters"), &MASTERS) }
    }
    #[wasm_bindgen(getter)]
    pub fn tags(&self) -> TagCollection {
        TagCollection { p: self.p.to(prop("tags"), &TAGS) }
    }
    #[wasm_bindgen(js_name = getSelectedSlides)]
    pub fn get_selected_slides(&self) -> SlideScopedCollection {
        SlideScopedCollection { p: self.p.to(method("getSelectedSlides", vec![]), &SLIDES) }
    }
    #[wasm_bindgen(js_name = getSelectedShapes)]
    pub fn get_selected_shapes(&self) -> ShapeScopedCollection {
        ShapeScopedCollection { p: self.p.to(method("getSelectedShapes", vec![]), &SHAPES) }
    }
    #[wasm_bindgen(js_name = getSelectedTextRange)]
    pub fn get_selected_text_range(&self) -> TextRange {
        TextRange { p: self.p.to(method("getSelectedTextRange", vec![]), &TEXT_RANGE) }
    }
    #[wasm_bindgen(js_name = setSelectedSlides)]
    pub fn set_selected_slides(&self, slide_ids: JsValue) {
        self.p.call("setSelectedSlides", vec![ids(&slide_ids)]);
    }
    /// `insertSlidesFromBase64(base64File, options?)`: options `formatting`, `sourceSlideIds`,
    /// `targetSlideId`.
    #[wasm_bindgen(js_name = insertSlidesFromBase64)]
    pub fn insert_slides_from_base64(&self, base64: String, opts: JsValue) {
        self.p.call("insertSlidesFromBase64", vec![json!(base64), options(&opts, &["formatting", "sourceSlideIds", "targetSlideId"])]);
    }
}

collection!(SlideCollection, Slide);

#[wasm_bindgen]
impl SlideCollection {
    /// `add(options?)`: `layoutId`, `slideMasterId`, and DeckCraft's `index` (0-based position).
    pub fn add(&self, opts: JsValue) {
        self.p.call("add", vec![options(&opts, &["layoutId", "slideMasterId", "index"])]);
    }
}

collection!(SlideScopedCollection, Slide);

class!(Slide);
ro!(Slide, id, "id");
ro!(Slide, index, "index");

#[wasm_bindgen]
impl Slide {
    #[wasm_bindgen(getter)]
    pub fn shapes(&self) -> ShapeCollection {
        ShapeCollection { p: self.p.to(prop("shapes"), &SHAPES) }
    }
    #[wasm_bindgen(getter)]
    pub fn layout(&self) -> SlideLayout {
        SlideLayout { p: self.p.to(prop("layout"), &LAYOUT) }
    }
    #[wasm_bindgen(getter = slideMaster)]
    pub fn slide_master(&self) -> SlideMaster {
        SlideMaster { p: self.p.to(prop("slideMaster"), &MASTER) }
    }
    #[wasm_bindgen(getter)]
    pub fn tags(&self) -> TagCollection {
        TagCollection { p: self.p.to(prop("tags"), &TAGS) }
    }
    pub fn delete(&self) {
        self.p.call("delete", vec![]);
    }
    #[wasm_bindgen(js_name = setSelectedShapes)]
    pub fn set_selected_shapes(&self, shape_ids: JsValue) {
        self.p.call("setSelectedShapes", vec![ids(&shape_ids)]);
    }
}

class!(SlideLayout);
ro!(SlideLayout, id, "id");
ro!(SlideLayout, name, "name");

#[wasm_bindgen]
impl SlideLayout {
    #[wasm_bindgen(getter)]
    pub fn shapes(&self) -> ShapeCollection {
        ShapeCollection { p: self.p.to(prop("shapes"), &SHAPES) }
    }
}

collection!(SlideLayoutCollection, SlideLayout);

class!(SlideMaster);
ro!(SlideMaster, id, "id");
ro!(SlideMaster, name, "name");

#[wasm_bindgen]
impl SlideMaster {
    #[wasm_bindgen(getter)]
    pub fn layouts(&self) -> SlideLayoutCollection {
        SlideLayoutCollection { p: self.p.to(prop("layouts"), &LAYOUTS) }
    }
    #[wasm_bindgen(getter)]
    pub fn shapes(&self) -> ShapeCollection {
        ShapeCollection { p: self.p.to(prop("shapes"), &SHAPES) }
    }
}

collection!(SlideMasterCollection, SlideMaster);

collection!(TagCollection, Tag);

#[wasm_bindgen]
impl TagCollection {
    pub fn add(&self, key: String, value: String) {
        self.p.call("add", vec![json!(key), json!(value)]);
    }
    pub fn delete(&self, key: String) {
        self.p.call("delete", vec![json!(key)]);
    }
}

class!(Tag);
ro!(Tag, key, "key");
rw!(Tag, value, set_value, "value");

// ---------------------------------------------------------------- shapes

collection!(ShapeCollection, Shape);

#[wasm_bindgen]
impl ShapeCollection {
    /// `addTextBox(text, options?)`: options `left`, `top`, `width`, `height` (points).
    #[wasm_bindgen(js_name = addTextBox)]
    pub fn add_text_box(&self, text: String, opts: JsValue) -> Shape {
        Shape { p: self.p.add("addTextBox", vec![json!(text), options(&opts, SHAPE_OPTIONS)]) }
    }
    /// `addGeometricShape(geometricShapeType, options?)`.
    #[wasm_bindgen(js_name = addGeometricShape)]
    pub fn add_geometric_shape(&self, kind: JsValue, opts: JsValue) -> Result<Shape, JsValue> {
        let kind = from_js(&kind);
        if !kind.is_string() {
            return Err(js_error(&OfficeError::new("InvalidArgument", "addGeometricShape needs a PowerPoint.GeometricShapeType")));
        }
        Ok(Shape { p: self.p.add("addGeometricShape", vec![enum_to_wire(&kind), options(&opts, SHAPE_OPTIONS)]) })
    }
    /// `addLine(connectorType?, options?)`: `Straight` by default.
    #[wasm_bindgen(js_name = addLine)]
    pub fn add_line(&self, kind: JsValue, opts: JsValue) -> Shape {
        let kind = match from_js(&kind) {
            Json::String(s) => enum_to_wire(&json!(s)),
            _ => json!("straight"),
        };
        Shape { p: self.p.add("addLine", vec![kind, options(&opts, SHAPE_OPTIONS)]) }
    }
}

collection!(ShapeScopedCollection, Shape);

class!(Shape);
ro!(Shape, id, "id");
rw!(Shape, name, set_name, "name");
ro!(Shape, kind, "type");
rw!(Shape, left, set_left, "left");
rw!(Shape, top, set_top, "top");
rw!(Shape, width, set_width, "width");
rw!(Shape, height, set_height, "height");

#[wasm_bindgen]
impl Shape {
    #[wasm_bindgen(getter = textFrame)]
    pub fn text_frame(&self) -> TextFrame {
        TextFrame { p: self.p.to(prop("textFrame"), &TEXT_FRAME) }
    }
    #[wasm_bindgen(getter)]
    pub fn fill(&self) -> ShapeFill {
        ShapeFill { p: self.p.to(prop("fill"), &FILL) }
    }
    #[wasm_bindgen(getter = lineFormat)]
    pub fn line_format(&self) -> ShapeLineFormat {
        ShapeLineFormat { p: self.p.to(prop("lineFormat"), &LINE) }
    }
    #[wasm_bindgen(getter)]
    pub fn tags(&self) -> TagCollection {
        TagCollection { p: self.p.to(prop("tags"), &TAGS) }
    }
    pub fn delete(&self) {
        self.p.call("delete", vec![]);
    }
}

class!(TextFrame);
ro!(TextFrame, has_text, "hasText");
rw!(TextFrame, word_wrap, set_word_wrap, "wordWrap");
rw!(TextFrame, auto_size_setting, set_auto_size_setting, "autoSizeSetting");
rw!(TextFrame, left_margin, set_left_margin, "leftMargin");
rw!(TextFrame, right_margin, set_right_margin, "rightMargin");
rw!(TextFrame, top_margin, set_top_margin, "topMargin");
rw!(TextFrame, bottom_margin, set_bottom_margin, "bottomMargin");
rw!(TextFrame, vertical_alignment, set_vertical_alignment, "verticalAlignment");

#[wasm_bindgen]
impl TextFrame {
    #[wasm_bindgen(getter = textRange)]
    pub fn text_range(&self) -> TextRange {
        TextRange { p: self.p.to(prop("textRange"), &TEXT_RANGE) }
    }
    /// `deleteText()`: the text range's text set to nothing.
    #[wasm_bindgen(js_name = deleteText)]
    pub fn delete_text(&self) {
        self.text_range().p.set("text", &JsValue::from_str(""));
    }
    /// `getParentShape()`: the shape this text frame belongs to.
    #[wasm_bindgen(js_name = getParentShape)]
    pub fn get_parent_shape(&self) -> Shape {
        let mut path = self.p.path.clone();
        path.pop();
        Shape { p: P { ctx: self.p.ctx.clone(), path, kind: &SHAPE } }
    }
}

class!(TextRange);
rw!(TextRange, text, set_text, "text");
ro!(TextRange, start, "start");
ro!(TextRange, length, "length");

#[wasm_bindgen]
impl TextRange {
    #[wasm_bindgen(getter)]
    pub fn font(&self) -> ShapeFont {
        ShapeFont { p: self.p.to(prop("font"), &FONT) }
    }
    #[wasm_bindgen(getter = paragraphFormat)]
    pub fn paragraph_format(&self) -> ParagraphFormat {
        ParagraphFormat { p: self.p.to(prop("paragraphFormat"), &PARAGRAPH) }
    }
    #[wasm_bindgen(js_name = getSubstring)]
    pub fn get_substring(&self, start: f64, length: JsValue) -> TextRange {
        let mut args = vec![num(start)];
        if let Some(l) = length.as_f64() {
            args.push(num(l));
        }
        TextRange { p: self.p.to(method("getSubstring", args), &TEXT_RANGE) }
    }
    #[wasm_bindgen(js_name = setSelected)]
    pub fn set_selected(&self) {
        self.p.call("setSelected", vec![]);
    }
}

class!(ShapeFont);
rw!(ShapeFont, bold, set_bold, "bold");
rw!(ShapeFont, italic, set_italic, "italic");
rw!(ShapeFont, underline, set_underline, "underline");
rw!(ShapeFont, size, set_size, "size");
rw!(ShapeFont, name, set_name, "name");
rw!(ShapeFont, color, set_color, "color");

class!(ParagraphFormat);
rw!(ParagraphFormat, horizontal_alignment, set_horizontal_alignment, "horizontalAlignment");

#[wasm_bindgen]
impl ParagraphFormat {
    #[wasm_bindgen(getter = bulletFormat)]
    pub fn bullet_format(&self) -> BulletFormat {
        BulletFormat { p: self.p.to(prop("bulletFormat"), &BULLETS) }
    }
}

class!(BulletFormat);
rw!(BulletFormat, visible, set_visible, "visible");

class!(ShapeFill);
ro!(ShapeFill, kind, "type");
rw!(ShapeFill, foreground_color, set_foreground_color, "foregroundColor");
rw!(ShapeFill, transparency, set_transparency, "transparency");

#[wasm_bindgen]
impl ShapeFill {
    #[wasm_bindgen(js_name = setSolidColor)]
    pub fn set_solid_color(&self, color: String) {
        self.p.call("setSolidColor", vec![json!(color)]);
    }
    pub fn clear(&self) {
        self.p.call("clear", vec![]);
    }
}

class!(ShapeLineFormat);
rw!(ShapeLineFormat, color, set_color, "color");
rw!(ShapeLineFormat, weight, set_weight, "weight");
rw!(ShapeLineFormat, visible, set_visible, "visible");
rw!(ShapeLineFormat, dash_style, set_dash_style, "dashStyle");
rw!(ShapeLineFormat, style, set_style, "style");
rw!(ShapeLineFormat, transparency, set_transparency, "transparency");

// ---------------------------------------------------------------- PowerPoint.run

/// `PowerPoint.run(batch)` (or `(object, batch)`): runs `batch(context)`, then sends what it
/// left queued; resolves with the batch's value.
fn powerpoint_run(a: JsValue, b: JsValue) -> Promise {
    let batch = a.clone().dyn_into::<Function>().or_else(|_| b.dyn_into::<Function>());
    future_to_promise(async move {
        let batch = batch.map_err(|_| js_error(&OfficeError::new("InvalidArgument", "PowerPoint.run needs a function")))?;
        let rc = RequestContext::new();
        let ctx = rc.p.ctx.clone();
        let mut value = batch.call1(&JsValue::UNDEFINED, &JsValue::from(rc))?;
        if let Ok(p) = value.clone().dyn_into::<Promise>() {
            value = JsFuture::from(p).await?;
        }
        if ctx.borrow().has_queued() {
            sync(ctx).await?;
        }
        Ok(value)
    })
}

// ---------------------------------------------------------------- Office (common API)

fn platform() -> String {
    let host = Reflect::get(&js_sys::global(), &"__craftOffice".into()).unwrap_or(JsValue::UNDEFINED);
    Reflect::get(&host, &"platform".into()).ok().and_then(|p| p.as_string()).unwrap_or_else(|| "PC".into())
}

fn obj(pairs: &[(&str, JsValue)]) -> Object {
    let o = Object::new();
    for (k, v) in pairs {
        let _ = Reflect::set(&o, &(*k).into(), v);
    }
    o
}

/// `{status, value, error, asyncContext}` for a callback.
fn async_result(r: Result<JsValue, OfficeError>, async_context: JsValue) -> JsValue {
    let o = match r {
        Ok(v) => obj(&[("status", "succeeded".into()), ("value", v)]),
        Err(e) => {
            let err = obj(&[("code", e.code.as_str().into()), ("name", e.code.as_str().into()), ("message", e.message.as_str().into())]);
            obj(&[("status", "failed".into()), ("value", JsValue::UNDEFINED), ("error", err.into())])
        }
    };
    let _ = Reflect::set(&o, &"asyncContext".into(), &async_context);
    o.into()
}

/// `(options?, callback?)` as Office.js's `*Async` methods take them.
fn options_and_callback(a: JsValue, b: JsValue) -> (JsValue, Option<Function>) {
    match a.clone().dyn_into::<Function>() {
        Ok(f) => (JsValue::UNDEFINED, Some(f)),
        Err(_) => (a, b.dyn_into::<Function>().ok()),
    }
}

fn finish(callback: Option<Function>, options: JsValue, r: Result<JsValue, OfficeError>) {
    if let Some(cb) = callback {
        let ctx = Reflect::get(&options, &"asyncContext".into()).unwrap_or(JsValue::UNDEFINED);
        let _ = cb.call1(&JsValue::UNDEFINED, &async_result(r, ctx));
    }
}

async fn document_call(method_name: &str, args: Vec<Json>) -> Result<Json, OfficeError> {
    let answer = send(vec![json!({"op": "call", "path": [prop("document")], "method": method_name, "args": args})]).await?;
    let results = crate::core::results(&answer)?;
    Ok(results.first().cloned().unwrap_or(Json::Null))
}

/// The image options `setSelectedDataAsync(base64, {coercionType: "image", …})` passes on.
const IMAGE_OPTIONS: &[&str] = &["imageLeft", "imageTop", "imageWidth", "imageHeight"];

/// Most function commands and event handlers kept.
const MAX_REGISTERED: usize = 1000;

fn document() -> Object {
    let get_selected = Closure::<dyn Fn(JsValue, JsValue, JsValue)>::new(|coercion: JsValue, a: JsValue, b: JsValue| {
        let (options, cb) = options_and_callback(a, b);
        let kind = coercion.as_string().unwrap_or_else(|| "text".into());
        spawn_local(async move {
            let r = match kind.as_str() {
                "text" | "slideRange" => document_call("getSelectedData", vec![json!(kind)]).await.map(|v| to_js(&v)),
                other => Err(not_supported(&format!("the coercion type `{other}`"))),
            };
            finish(cb, options, r);
        });
    });
    let set_selected = Closure::<dyn Fn(JsValue, JsValue, JsValue)>::new(|data: JsValue, a: JsValue, b: JsValue| {
        let (opts, cb) = options_and_callback(a, b);
        let data = from_js(&data);
        let coercion = Reflect::get(&opts, &"coercionType".into()).ok().and_then(|c| c.as_string()).unwrap_or_else(|| "text".into());
        let mut image = options(&opts, IMAGE_OPTIONS);
        image["coercionType"] = json!("image");
        let options = opts;
        spawn_local(async move {
            let r = match (coercion.as_str(), data.is_string()) {
                ("image", true) => document_call("setSelectedData", vec![data, image]).await.map(|_| JsValue::UNDEFINED),
                ("image", false) => Err(OfficeError::new("InvalidArgument", "an image is given as base64 text")),
                ("text", true) => document_call("setSelectedData", vec![data]).await.map(|_| JsValue::UNDEFINED),
                ("text", false) => Err(not_supported("setting the selection to anything but text")),
                (other, _) => Err(not_supported(&format!("the coercion type `{other}`"))),
            };
            finish(cb, options, r);
        });
    });
    // `goToByIdAsync(id, goToType, options?, callback?)`: slides by id (`slide`) or by index.
    let go_to = Closure::<dyn Fn(JsValue, JsValue, JsValue, JsValue)>::new(|id: JsValue, kind: JsValue, a: JsValue, b: JsValue| {
        let (options, cb) = options_and_callback(a, b);
        let id = from_js(&id);
        let kind = kind.as_string().unwrap_or_else(|| "slide".into());
        spawn_local(async move {
            let r = document_call("goToById", vec![id, json!(kind)]).await.map(|_| JsValue::UNDEFINED);
            finish(cb, options, r);
        });
    });
    let doc = obj(&[
        ("getSelectedDataAsync", get_selected.into_js_value()),
        ("setSelectedDataAsync", set_selected.into_js_value()),
        ("goToByIdAsync", go_to.into_js_value()),
        ("addHandlerAsync", add_handler()),
        ("removeHandlerAsync", remove_handler()),
        ("getFileAsync", unsupported_async("getFileAsync")),
        ("getFilePropertiesAsync", unsupported_async("getFilePropertiesAsync")),
    ]);
    let bindings = obj(&[
        ("addFromNamedItemAsync", unsupported_async("bindings")),
        ("addFromPromptAsync", unsupported_async("bindings")),
        ("addFromSelectionAsync", unsupported_async("bindings")),
        ("getAllAsync", unsupported_async("bindings")),
        ("getByIdAsync", unsupported_async("bindings")),
        ("releaseByIdAsync", unsupported_async("bindings")),
    ]);
    let _ = Reflect::set(&doc, &"bindings".into(), &bindings.into());
    let _ = Reflect::set(&doc, &"settings".into(), &settings().into());
    let _ = Reflect::set(&doc, &"mode".into(), &"readWrite".into());
    doc
}

/// Calls `callback(asyncResult)` later, as Office does (never before the method returns).
fn finish_later(callback: Option<Function>, options: JsValue, r: Result<JsValue, OfficeError>) {
    if callback.is_some() {
        spawn_local(async move { finish(callback, options, r) });
    }
}

fn not_supported(what: &str) -> OfficeError {
    OfficeError::new("NotSupported", format!("{what} isn't supported by DeckCraft yet"))
}

/// A common-API `*Async` method DeckCraft doesn't have: its callback (the last function among the
/// arguments) gets a failed result with code `NotSupported`, instead of the page getting a `TypeError`.
fn unsupported_async(what: &'static str) -> JsValue {
    Closure::<dyn Fn(JsValue, JsValue, JsValue, JsValue)>::new(move |a: JsValue, b: JsValue, c: JsValue, d: JsValue| {
        let args = [a, b, c, d];
        let cb = args.iter().rev().find_map(|v| v.clone().dyn_into::<Function>().ok());
        let options = args.iter().rev().find(|v| v.is_object() && !v.is_function()).cloned().unwrap_or(JsValue::UNDEFINED);
        finish_later(cb, options, Err(not_supported(what)));
    })
    .into_js_value()
}

/// A Promise-based method DeckCraft doesn't have: rejects with `NotImplemented`.
fn rejecting(what: &'static str) -> JsValue {
    Closure::<dyn Fn() -> Promise>::new(move || Promise::reject(&not_implemented(what))).into_js_value()
}

const SELECTION_CHANGED: &str = "documentSelectionChanged";

/// `addHandlerAsync(eventType, handler, options?, callback?)`: `DocumentSelectionChanged` is
/// delivered (`__craftOfficeEvent`); other events answer `NotSupported`.
fn add_handler() -> JsValue {
    Closure::<dyn Fn(JsValue, JsValue, JsValue, JsValue)>::new(|kind: JsValue, handler: JsValue, a: JsValue, b: JsValue| {
        let (options, cb) = options_and_callback(a, b);
        let kind = kind.as_string().unwrap_or_default();
        let r = if kind != SELECTION_CHANGED {
            Err(not_supported(&format!("the event `{kind}`")))
        } else {
            match handler.dyn_into::<Function>() {
                Ok(f) => SELECTION_HANDLERS.with(|h| {
                    let mut h = h.borrow_mut();
                    if h.len() >= MAX_REGISTERED {
                        return Err(OfficeError::new("InvalidArgument", "too many handlers"));
                    }
                    h.push(f);
                    Ok(JsValue::UNDEFINED)
                }),
                Err(_) => Err(OfficeError::new("InvalidArgument", "addHandlerAsync needs a handler function")),
            }
        };
        finish_later(cb, options, r);
    })
    .into_js_value()
}

/// `removeHandlerAsync(eventType, options?, callback?)`: `options.handler`, or all of them.
fn remove_handler() -> JsValue {
    Closure::<dyn Fn(JsValue, JsValue, JsValue)>::new(|kind: JsValue, a: JsValue, b: JsValue| {
        let (options, cb) = options_and_callback(a, b);
        let kind = kind.as_string().unwrap_or_default();
        let r = if kind == SELECTION_CHANGED {
            let which = Reflect::get(&options, &"handler".into()).ok().and_then(|f| f.dyn_into::<Function>().ok());
            SELECTION_HANDLERS.with(|h| match which {
                Some(f) => h.borrow_mut().retain(|g| !Object::is(g, &f)),
                None => h.borrow_mut().clear(),
            });
            Ok(JsValue::UNDEFINED)
        } else {
            Err(not_supported(&format!("the event `{kind}`")))
        };
        finish_later(cb, options, r);
    })
    .into_js_value()
}

/// `globalThis.__craftOfficeEvent({"type"})`: an event from the host (`documentSelectionChanged`)
/// for the page's handlers.
fn host_event(text: String) {
    let kind = serde_json::from_str::<Json>(&text).ok().and_then(|v| v.get("type").and_then(Json::as_str).map(str::to_string));
    if kind.as_deref() != Some(SELECTION_CHANGED) {
        return;
    }
    let office = Reflect::get(&js_sys::global(), &"Office".into()).unwrap_or(JsValue::UNDEFINED);
    let document = Reflect::get(&office, &"context".into()).and_then(|c| Reflect::get(&c, &"document".into())).unwrap_or(JsValue::UNDEFINED);
    let args = obj(&[("type", SELECTION_CHANGED.into()), ("document", document)]);
    for f in SELECTION_HANDLERS.with(|h| h.borrow().clone()) {
        if let Err(e) = f.call1(&JsValue::UNDEFINED, &args) {
            log_error(&e);
        }
    }
}

/// Sends the host an event (`{"event": "ready"}`, `{"event": "functionDone", …}`); no answer.
fn post_event(msg: Json) {
    let host = Reflect::get(&js_sys::global(), &"__craftOffice".into()).unwrap_or(JsValue::UNDEFINED);
    if let Some(post) = Reflect::get(&host, &"post".into()).ok().and_then(|f| f.dyn_into::<Function>().ok()) {
        let _ = post.call1(&host, &msg.to_string().into());
    }
}

/// `console.error(e)`, where there is a console.
fn log_error(e: &JsValue) {
    let console = Reflect::get(&js_sys::global(), &"console".into()).unwrap_or(JsValue::UNDEFINED);
    if let Some(error) = Reflect::get(&console, &"error".into()).ok().and_then(|f| f.dyn_into::<Function>().ok()) {
        let _ = error.call1(&console, e);
    }
}

// ---------------------------------------------------------------- function commands

/// Resolves once the page has loaded (its scripts have run, so their functions are associated);
/// at once where there is no document (tests).
async fn page_loaded() {
    let global = js_sys::global();
    let doc = Reflect::get(&global, &"document".into()).unwrap_or(JsValue::UNDEFINED);
    if !doc.is_object() || Reflect::get(&doc, &"readyState".into()).ok().and_then(|s| s.as_string()).as_deref() == Some("complete") {
        return;
    }
    let loaded = Promise::new(&mut |resolve, _| match Reflect::get(&global, &"addEventListener".into()).and_then(|f| f.dyn_into::<Function>()) {
        Ok(add) => {
            let _ = add.call2(&global, &"load".into(), &resolve);
        }
        Err(_) => {
            let _ = resolve.call0(&JsValue::UNDEFINED);
        }
    });
    let _ = JsFuture::from(loaded).await;
}

/// The function an `ExecuteFunction` command names: associated with `Office.actions.associate`,
/// else a global function (`name` or `a.b.name`), as function files without `associate` have.
fn find_function(name: &str) -> Option<Function> {
    if let Some(f) = ACTIONS.with(|a| a.borrow().get(name).cloned()) {
        return Some(f);
    }
    let mut at: JsValue = js_sys::global().into();
    for part in name.split('.').take(8) {
        at = Reflect::get(&at, &part.into()).ok().filter(|v| v.is_object() || v.is_function())?;
    }
    at.dyn_into::<Function>().ok()
}

fn error_text(e: &JsValue) -> String {
    Reflect::get(e, &"message".into()).ok().and_then(|m| m.as_string()).or_else(|| e.as_string()).unwrap_or_else(|| "the function failed".into())
}

/// `globalThis.__craftOfficeRunFunction({"call", "name", "control"})`: runs a function command
/// once the library and the page are ready. The function gets `event.source.id` (the control's id
/// in the manifest) and `event.completed()`, which tells the host it has finished
/// (`{"event": "functionDone", "call"}`, with `error` when it couldn't run or failed).
fn run_function(text: String) {
    let Some(call) = FunctionCall::parse(&text) else {
        if let Some(id) = serde_json::from_str::<Json>(&text).ok().and_then(|v| v.get("call").cloned()) {
            post_event(json!({"event": "functionDone", "call": id, "error": "no function named"}));
        }
        return;
    };
    let ready = READY.with(|r| r.borrow().clone());
    spawn_local(async move {
        if let Some(ready) = ready {
            let _ = JsFuture::from(ready).await;
        }
        page_loaded().await;
        let done = Rc::new(Cell::new(false));
        let fail = {
            let (done, call) = (done.clone(), call.clone());
            move |message: String| {
                if !done.replace(true) {
                    post_event(call.done(Some(&message)));
                }
            }
        };
        let Some(f) = find_function(&call.name) else {
            fail(format!("the add-in's function file has no function `{}`", call.name));
            return;
        };
        let completed = {
            let (done, call) = (done.clone(), call.clone());
            Closure::<dyn Fn(JsValue)>::new(move |_options: JsValue| {
                if !done.replace(true) {
                    post_event(call.done(None));
                }
            })
            .into_js_value()
        };
        let event = obj(&[("source", obj(&[("id", call.control.as_str().into())]).into()), ("completed", completed)]);
        match f.call1(&JsValue::UNDEFINED, &event) {
            Ok(v) => {
                if let Ok(p) = v.dyn_into::<Promise>()
                    && let Err(e) = JsFuture::from(p).await
                {
                    fail(error_text(&e));
                }
            }
            Err(e) => {
                log_error(&e);
                fail(error_text(&e));
            }
        }
    });
}

/// `Office.actions`: `associate(name, fn)` for function commands; the keyboard-shortcut methods
/// reject with `NotImplemented`.
fn actions() -> Object {
    let associate = Closure::<dyn Fn(JsValue, JsValue)>::new(|name: JsValue, f: JsValue| {
        if let (Some(name), Ok(f)) = (name.as_string(), f.dyn_into::<Function>()) {
            ACTIONS.with(|a| {
                let mut a = a.borrow_mut();
                if a.len() < MAX_REGISTERED || a.contains_key(&name) {
                    a.insert(name, f);
                }
            });
        }
    });
    obj(&[
        ("associate", associate.into_js_value()),
        ("areShortcutsInUse", rejecting("Office.actions.areShortcutsInUse")),
        ("getShortcuts", rejecting("Office.actions.getShortcuts")),
        ("replaceShortcuts", rejecting("Office.actions.replaceShortcuts")),
    ])
}

/// `Office.context.document.settings`: kept here, saved to the host with `saveAsync`.
fn settings() -> Object {
    let get = Closure::<dyn Fn(String) -> JsValue>::new(|name: String| SETTINGS.with(|s| s.borrow().get(&name).map(to_js).unwrap_or(JsValue::NULL)));
    let set = Closure::<dyn Fn(String, JsValue)>::new(|name: String, v: JsValue| {
        let v = from_js(&v);
        SETTINGS.with(|s| s.borrow_mut().insert(name, v));
    });
    let remove = Closure::<dyn Fn(String)>::new(|name: String| {
        SETTINGS.with(|s| s.borrow_mut().remove(&name));
    });
    let save = Closure::<dyn Fn(JsValue, JsValue)>::new(|a: JsValue, b: JsValue| {
        let (options, cb) = options_and_callback(a, b);
        let all = SETTINGS.with(|s| Json::Object(s.borrow().clone()));
        spawn_local(async move {
            let r = match send(vec![json!({"op": "call", "path": [prop("settings")], "method": "save", "args": [all]})]).await {
                Ok(answer) => crate::core::results(&answer).map(|_| JsValue::UNDEFINED),
                Err(e) => Err(e),
            };
            finish(cb, options, r);
        });
    });
    obj(&[("get", get.into_js_value()), ("set", set.into_js_value()), ("remove", remove.into_js_value()), ("saveAsync", save.into_js_value())])
}

/// Reads the add-in's saved settings from the host (at start).
async fn load_settings() {
    if let Ok(answer) = send(vec![json!({"op": "load", "path": [prop("settings")], "props": []})]).await
        && let Some(Json::Object(saved)) = answer.get("results").and_then(|r| r.get(0)).cloned()
    {
        SETTINGS.with(|s| *s.borrow_mut() = saved);
    }
}

/// An enum object: `{Key: "Value"}` for each value.
fn enums(pairs: &[(&str, &str)]) -> JsValue {
    let o = Object::new();
    for (k, v) in pairs {
        let _ = Reflect::set(&o, &(*k).into(), &(*v).into());
    }
    o.into()
}

/// A PowerPoint enum: camelCase keys, PascalCase values (`rightArrow: "RightArrow"`).
fn pp_enum(keys: &[&str]) -> JsValue {
    let o = Object::new();
    for k in keys {
        let v = enum_from_wire(&json!(k));
        let _ = Reflect::set(&o, &(*k).into(), &v.as_str().unwrap_or(k).into());
    }
    o.into()
}

fn office() -> Object {
    let platform = platform();
    let info = obj(&[("host", "PowerPoint".into()), ("platform", platform.as_str().into())]);
    let on_ready = {
        let info = info.clone();
        Closure::<dyn Fn(JsValue) -> Promise>::new(move |cb: JsValue| {
            let ready = READY.with(|r| r.borrow().clone()).unwrap_or_else(|| Promise::resolve(&JsValue::UNDEFINED));
            let info = info.clone();
            future_to_promise(async move {
                let _ = JsFuture::from(ready).await;
                if let Ok(f) = cb.dyn_into::<Function>() {
                    let _ = f.call1(&JsValue::UNDEFINED, &info);
                }
                Ok(info.into())
            })
        })
    };
    let is_set = Closure::<dyn Fn(String, JsValue) -> bool>::new(|name: String, v: JsValue| {
        let v = v.as_string().or_else(|| v.as_f64().map(|f| format!("{f}")));
        is_set_supported(&name, v.as_deref())
    });
    let dialog = Closure::<dyn Fn(JsValue, JsValue, JsValue)>::new(|_url: JsValue, a: JsValue, b: JsValue| {
        let (options, cb) = options_and_callback(a, b);
        finish_later(cb, options, Err(OfficeError::not_implemented("displayDialogAsync")));
    });
    let context = obj(&[
        ("host", "PowerPoint".into()),
        ("diagnostics", obj(&[("host", "PowerPoint".into()), ("platform", platform.as_str().into()), ("version", "16.0 (DeckCraft)".into())]).into()),
        ("requirements", obj(&[("isSetSupported", is_set.into_js_value())]).into()),
        ("document", document().into()),
        (
            "ui",
            obj(&[("displayDialogAsync", dialog.into_js_value()), ("messageParent", Closure::<dyn Fn(JsValue)>::new(|_| {}).into_js_value())]).into(),
        ),
        ("displayLanguage", "en-US".into()),
        ("contentLanguage", "en-US".into()),
        ("platform", platform.as_str().into()),
    ]);
    let addin = obj(&[
        ("showAsTaskpane", rejecting("Office.addin.showAsTaskpane")),
        ("hide", rejecting("Office.addin.hide")),
        ("getStartupBehavior", rejecting("Office.addin.getStartupBehavior")),
        ("setStartupBehavior", rejecting("Office.addin.setStartupBehavior")),
        ("onVisibilityModeChanged", rejecting("Office.addin.onVisibilityModeChanged")),
    ]);
    let ribbon = obj(&[
        ("requestUpdate", rejecting("Office.ribbon.requestUpdate")),
        ("requestCreateControls", rejecting("Office.ribbon.requestCreateControls")),
    ]);
    let mut members = vec![
        ("onReady", on_ready.into_js_value()),
        ("context", context.into()),
        ("actions", actions().into()),
        ("addin", addin.into()),
        ("ribbon", ribbon.into()),
    ];
    members.extend(office_enums());
    let o = obj(&members);
    lock(&o);
    define_initialize(&o);
    o
}

/// The common API's enumerations, as the API reference lists them.
fn office_enums() -> Vec<(&'static str, JsValue)> {
    vec![
        ("AsyncResultStatus", enums(&[("Succeeded", "succeeded"), ("Failed", "failed")])),
        (
            "CoercionType",
            enums(&[
                ("Text", "text"),
                ("Matrix", "matrix"),
                ("Table", "table"),
                ("Html", "html"),
                ("Ooxml", "ooxml"),
                ("SlideRange", "slideRange"),
                ("Image", "image"),
                ("XmlSvg", "xmlSvg"),
            ]),
        ),
        (
            "HostType",
            enums(&[
                ("Word", "Word"),
                ("Excel", "Excel"),
                ("PowerPoint", "PowerPoint"),
                ("Outlook", "Outlook"),
                ("OneNote", "OneNote"),
                ("Project", "Project"),
                ("Access", "Access"),
            ]),
        ),
        (
            "PlatformType",
            enums(&[
                ("PC", "PC"),
                ("OfficeOnline", "OfficeOnline"),
                ("Mac", "Mac"),
                ("iOS", "iOS"),
                ("Android", "Android"),
                ("Universal", "Universal"),
            ]),
        ),
        (
            "EventType",
            enums(&[
                ("ActiveViewChanged", "activeViewChanged"),
                ("AppointmentTimeChanged", "olkAppointmentTimeChanged"),
                ("AttachmentsChanged", "olkAttachmentsChanged"),
                ("BindingDataChanged", "bindingDataChanged"),
                ("BindingSelectionChanged", "bindingSelectionChanged"),
                ("DialogEventReceived", "dialogEventReceived"),
                ("DialogMessageReceived", "dialogMessageReceived"),
                ("DialogParentMessageReceived", "dialogParentMessageReceived"),
                ("DocumentSelectionChanged", "documentSelectionChanged"),
                ("DragAndDropEvent", "olkDragAndDropEvent"),
                ("EnhancedLocationsChanged", "olkEnhancedLocationsChanged"),
                ("InfobarClicked", "olkInfobarClicked"),
                ("ItemChanged", "olkItemSelectedChanged"),
                ("NodeDeleted", "nodeDeleted"),
                ("NodeInserted", "nodeInserted"),
                ("NodeReplaced", "nodeReplaced"),
                ("OfficeThemeChanged", "officeThemeChanged"),
                ("RecipientsChanged", "olkRecipientsChanged"),
                ("RecurrenceChanged", "olkRecurrenceChanged"),
                ("ResourceSelectionChanged", "resourceSelectionChanged"),
                ("SelectedItemsChanged", "olkSelectedItemsChanged"),
                ("SensitivityLabelChanged", "olkSensitivityLabelChanged"),
                ("SettingsChanged", "settingsChanged"),
                ("SpamReporting", "olkSpamReporting"),
                ("TaskSelectionChanged", "taskSelectionChanged"),
                ("ViewSelectionChanged", "viewSelectionChanged"),
            ]),
        ),
        ("Index", enums(&[("First", "first"), ("Last", "last"), ("Next", "next"), ("Previous", "previous")])),
        ("GoToType", enums(&[("Binding", "binding"), ("NamedItem", "namedItem"), ("Slide", "slide"), ("Index", "index")])),
        ("BindingType", enums(&[("Text", "text"), ("Matrix", "matrix"), ("Table", "table")])),
        ("StartupBehavior", enums(&[("none", "None"), ("load", "Load")])),
        ("VisibilityMode", enums(&[("hidden", "Hidden"), ("taskpane", "Taskpane")])),
        ("InitializationReason", enums(&[("Inserted", "inserted"), ("DocumentOpened", "documentOpened")])),
        ("ValueFormat", enums(&[("Unformatted", "unformatted"), ("Formatted", "formatted")])),
        ("FilterType", enums(&[("All", "all"), ("OnlyVisible", "onlyVisible")])),
        ("SelectionMode", enums(&[("Default", "default"), ("Selected", "selected"), ("None", "none")])),
        ("DocumentMode", enums(&[("ReadOnly", "readOnly"), ("ReadWrite", "readWrite")])),
        ("ActiveView", enums(&[("Read", "read"), ("Edit", "edit")])),
        ("FileType", enums(&[("Text", "text"), ("Compressed", "compressed"), ("Pdf", "pdf")])),
        ("Table", enums(&[("All", "all"), ("Data", "data"), ("Headers", "headers")])),
    ]
}

/// Calls the page's `Office.initialize` (later, as Office does).
fn call_initialize() {
    if let Some(f) = INITIALIZE.with(|i| i.borrow().clone()) {
        spawn_local(async move {
            if let Err(e) = f.call1(&JsValue::UNDEFINED, &"inserted".into()) {
                log_error(&e);
            }
        });
    }
}

/// `Office.initialize`: kept, and called once the library is ready, or at once if it already is.
fn define_initialize(office: &Object) {
    let get = Closure::<dyn Fn() -> JsValue>::new(|| INITIALIZE.with(|i| i.borrow().clone().map(JsValue::from).unwrap_or(JsValue::UNDEFINED)));
    let set = Closure::<dyn Fn(JsValue)>::new(|v: JsValue| {
        let f = v.dyn_into::<Function>().ok();
        let call_now = f.is_some() && IS_READY.with(Cell::get);
        INITIALIZE.with(|i| *i.borrow_mut() = f);
        if call_now {
            call_initialize();
        }
    });
    let desc = obj(&[("get", get.into_js_value()), ("set", set.into_js_value()), ("configurable", false.into()), ("enumerable", true.into())]);
    let _ = Reflect::define_property(office, &"initialize".into(), &desc);
}

/// Makes `o`'s properties read-only (a later script can't swap `Office.onReady` and the rest).
fn lock(o: &Object) {
    for key in Object::keys(o).iter() {
        let desc = obj(&[("writable", false.into()), ("configurable", false.into())]);
        let _ = Reflect::define_property(o, &key, &desc);
    }
}

/// `PowerPoint.GeometricShapeType`'s values (camelCase), from the public API reference.
const GEOMETRIC_SHAPES: &[&str] = &[
    "lineInverse",
    "triangle",
    "rightTriangle",
    "rectangle",
    "diamond",
    "parallelogram",
    "trapezoid",
    "nonIsoscelesTrapezoid",
    "pentagon",
    "hexagon",
    "heptagon",
    "octagon",
    "decagon",
    "dodecagon",
    "star4",
    "star5",
    "star6",
    "star7",
    "star8",
    "star10",
    "star12",
    "star16",
    "star24",
    "star32",
    "roundRectangle",
    "round1Rectangle",
    "round2SameRectangle",
    "round2DiagonalRectangle",
    "snipRoundRectangle",
    "snip1Rectangle",
    "snip2SameRectangle",
    "snip2DiagonalRectangle",
    "plaque",
    "ellipse",
    "teardrop",
    "homePlate",
    "chevron",
    "pieWedge",
    "pie",
    "blockArc",
    "donut",
    "noSmoking",
    "rightArrow",
    "leftArrow",
    "upArrow",
    "downArrow",
    "stripedRightArrow",
    "notchedRightArrow",
    "bentUpArrow",
    "leftRightArrow",
    "upDownArrow",
    "leftUpArrow",
    "leftRightUpArrow",
    "quadArrow",
    "leftArrowCallout",
    "rightArrowCallout",
    "upArrowCallout",
    "downArrowCallout",
    "leftRightArrowCallout",
    "upDownArrowCallout",
    "quadArrowCallout",
    "bentArrow",
    "uturnArrow",
    "circularArrow",
    "leftCircularArrow",
    "leftRightCircularArrow",
    "curvedRightArrow",
    "curvedLeftArrow",
    "curvedUpArrow",
    "curvedDownArrow",
    "swooshArrow",
    "cube",
    "can",
    "lightningBolt",
    "heart",
    "sun",
    "moon",
    "smileyFace",
    "irregularSeal1",
    "irregularSeal2",
    "foldedCorner",
    "bevel",
    "frame",
    "halfFrame",
    "corner",
    "diagonalStripe",
    "chord",
    "arc",
    "leftBracket",
    "rightBracket",
    "leftBrace",
    "rightBrace",
    "bracketPair",
    "bracePair",
    "callout1",
    "callout2",
    "callout3",
    "accentCallout1",
    "accentCallout2",
    "accentCallout3",
    "borderCallout1",
    "borderCallout2",
    "borderCallout3",
    "accentBorderCallout1",
    "accentBorderCallout2",
    "accentBorderCallout3",
    "wedgeRectCallout",
    "wedgeRRectCallout",
    "wedgeEllipseCallout",
    "cloudCallout",
    "cloud",
    "ribbon",
    "ribbon2",
    "ellipseRibbon",
    "ellipseRibbon2",
    "leftRightRibbon",
    "verticalScroll",
    "horizontalScroll",
    "wave",
    "doubleWave",
    "plus",
    "flowChartProcess",
    "flowChartDecision",
    "flowChartInputOutput",
    "flowChartPredefinedProcess",
    "flowChartInternalStorage",
    "flowChartDocument",
    "flowChartMultidocument",
    "flowChartTerminator",
    "flowChartPreparation",
    "flowChartManualInput",
    "flowChartManualOperation",
    "flowChartConnector",
    "flowChartPunchedCard",
    "flowChartPunchedTape",
    "flowChartSummingJunction",
    "flowChartOr",
    "flowChartCollate",
    "flowChartSort",
    "flowChartExtract",
    "flowChartMerge",
    "flowChartOfflineStorage",
    "flowChartOnlineStorage",
    "flowChartMagneticTape",
    "flowChartMagneticDisk",
    "flowChartMagneticDrum",
    "flowChartDisplay",
    "flowChartDelay",
    "flowChartAlternateProcess",
    "flowChartOffpageConnector",
    "actionButtonBlank",
    "actionButtonHome",
    "actionButtonHelp",
    "actionButtonInformation",
    "actionButtonForwardNext",
    "actionButtonBackPrevious",
    "actionButtonEnd",
    "actionButtonBeginning",
    "actionButtonReturn",
    "actionButtonDocument",
    "actionButtonSound",
    "actionButtonMovie",
    "gear6",
    "gear9",
    "funnel",
    "mathPlus",
    "mathMinus",
    "mathMultiply",
    "mathDivide",
    "mathEqual",
    "mathNotEqual",
    "cornerTabs",
    "squareTabs",
    "plaqueTabs",
    "chartX",
    "chartStar",
    "chartPlus",
];

fn powerpoint() -> Object {
    let run = Closure::<dyn Fn(JsValue, JsValue) -> Promise>::new(powerpoint_run);
    let create = Closure::<dyn Fn(JsValue) -> Promise>::new(|_: JsValue| Promise::reject(&not_implemented("PowerPoint.createPresentation")));
    // `new PowerPoint.RequestContext()`: the exported class, reached through an instance.
    let request_context = Reflect::get(&JsValue::from(RequestContext::new()), &"constructor".into()).unwrap_or(JsValue::UNDEFINED);
    let o = obj(&[("run", run.into_js_value()), ("createPresentation", create.into_js_value()), ("RequestContext", request_context)]);
    let tables: [(&str, &[&str]); 11] = [
        ("GeometricShapeType", GEOMETRIC_SHAPES),
        ("ConnectorType", &["straight", "elbow", "curve"]),
        (
            "ShapeType",
            &[
                "unsupported",
                "image",
                "geometricShape",
                "group",
                "line",
                "table",
                "callout",
                "chart",
                "contentApp",
                "diagram",
                "freeform",
                "graphic",
                "ink",
                "media",
                "model3D",
                "ole",
                "placeholder",
                "smartArt",
                "textBox",
            ],
        ),
        ("ShapeFillType", &["noFill", "solid", "gradient", "pattern", "pictureAndTexture", "slideBackground"]),
        ("ShapeAutoSize", &["autoSizeNone", "autoSizeTextToFitShape", "autoSizeShapeToFitText"]),
        (
            "ShapeLineDashStyle",
            &[
                "dash",
                "dashDot",
                "dashDotDot",
                "longDash",
                "longDashDot",
                "roundDot",
                "solid",
                "squareDot",
                "longDashDotDot",
                "systemDash",
                "systemDot",
                "systemDashDot",
            ],
        ),
        ("ShapeLineStyle", &["single", "thickBetweenThin", "thickThin", "thinThick", "thinThin"]),
        (
            "ShapeUnderlineStyle",
            &[
                "none",
                "single",
                "double",
                "heavy",
                "dotted",
                "dottedHeavy",
                "dash",
                "dashHeavy",
                "dashLong",
                "dashLongHeavy",
                "dotDash",
                "dotDashHeavy",
                "dotDotDash",
                "dotDotDashHeavy",
                "wavy",
                "wavyHeavy",
                "wavyDouble",
            ],
        ),
        ("TextVerticalAlignment", &["top", "middle", "bottom", "topCentered", "middleCentered", "bottomCentered"]),
        ("ParagraphHorizontalAlignment", &["left", "center", "right", "justify", "justifyLow", "distributed", "thaiDistributed"]),
        ("InsertSlideFormatting", &["keepSourceFormatting", "useDestinationTheme"]),
    ];
    for (name, keys) in tables {
        let _ = Reflect::set(&o, &name.into(), &pp_enum(keys));
    }
    lock(&o);
    o
}

fn office_extension() -> Object {
    let codes = Object::new();
    for c in [
        "AccessDenied",
        "GeneralException",
        "InvalidArgument",
        "ItemNotFound",
        "NotImplemented",
        "NotSupported",
        "PropertyNotLoaded",
        "ValueNotLoaded",
    ] {
        let _ = Reflect::set(&codes, &c.into(), &c.into());
    }
    // `OfficeExtension.Error` is `Error`: every error this library throws is one.
    let error = Reflect::get(&js_sys::global(), &"Error".into()).unwrap_or(JsValue::UNDEFINED);
    let o = obj(&[("Error", error), ("ErrorCodes", codes.into())]);
    lock(&o);
    o
}

/// Defines a global the page uses. With the bootstrap (a task pane), its property nobody can
/// replace takes the object (`__craftOfficeAdopt`); without (tests), a property that isn't writable.
fn define(name: &str, value: &JsValue) {
    let global: Object = js_sys::global();
    if let Ok(adopt) = Reflect::get(&global, &"__craftOfficeAdopt".into()).and_then(|f| f.dyn_into::<Function>()) {
        let _ = adopt.call2(&JsValue::UNDEFINED, &name.into(), value);
        return;
    }
    let desc = obj(&[("value", value.clone()), ("writable", false.into()), ("configurable", false.into()), ("enumerable", true.into())]);
    let _ = Reflect::define_property(&global, &name.into(), &desc);
}

/// Defines one of the library's entry points for the host (not enumerable, not replaceable).
fn define_entry(name: &str, f: JsValue) {
    let desc = obj(&[("value", f), ("writable", false.into()), ("configurable", false.into()), ("enumerable", false.into())]);
    let _ = Reflect::define_property(&js_sys::global(), &name.into(), &desc);
}

/// Defines `Office`, `PowerPoint`, `OfficeExtension` and `OfficeRuntime` (not replaceable: a
/// later `office.js` can't take over), the host's entry points (`__craftOfficeReply`,
/// `__craftOfficeRunFunction`, `__craftOfficeEvent`), and loads the add-in's settings; `onReady`
/// resolves after that, `Office.initialize` is called and the host hears `{"event": "ready"}`.
/// Called again, it only starts over the settings, the handlers and `onReady`.
fn install_globals() {
    if !INSTALLED.with(|i| i.replace(true)) {
        define("Office", &office().into());
        define("PowerPoint", &powerpoint().into());
        define("OfficeExtension", &office_extension().into());
        define("OfficeRuntime", &Object::new().into());
        define_entry("__craftOfficeReply", Closure::<dyn Fn(String)>::new(reply).into_js_value());
        define_entry("__craftOfficeRunFunction", Closure::<dyn Fn(String)>::new(run_function).into_js_value());
        define_entry("__craftOfficeEvent", Closure::<dyn Fn(String)>::new(host_event).into_js_value());
    }
    SETTINGS.with(|s| s.borrow_mut().clear());
    IS_READY.with(|r| r.set(false));
    SELECTION_HANDLERS.with(|h| h.borrow_mut().clear());
    let ready = future_to_promise(async {
        load_settings().await;
        IS_READY.with(|r| r.set(true));
        // A page that set `Office.initialize` gets it called, as Office does.
        call_initialize();
        // The host may call function commands now.
        post_event(json!({"event": "ready"}));
        Ok(JsValue::UNDEFINED)
    });
    READY.with(|r| *r.borrow_mut() = Some(ready));
}

/// Installs the library in a task pane that talks to DeckCraft through
/// `globalThis.__craftOffice.post`.
#[cfg_attr(not(test), wasm_bindgen(start))]
pub fn install() {
    TRANSPORT.with(|t| *t.borrow_mut() = None);
    install_globals();
}

/// Installs the library with a synchronous transport (tests: a host in the same process).
pub fn install_with_transport(t: impl Fn(String) -> String + 'static) {
    TRANSPORT.with(|tr| *tr.borrow_mut() = Some(Rc::new(t)));
    install_globals();
}

#[cfg(test)]
mod tests;
