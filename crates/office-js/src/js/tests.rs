//! The library as an add-in sees it — `PowerPoint.run`, proxies, `context.sync()`, the common
//! API — against a fake host that answers `office.sync` per the bridge contract (an in-memory
//! deck, all or nothing per sync). Run with `wasm-bindgen-test-runner` (Node).

use std::cell::RefCell;

use js_sys::{Array, Function, Promise, Reflect};
use serde_json::{Map, Value as Json, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, future_to_promise, spawn_local};
use wasm_bindgen_test::wasm_bindgen_test;

use super::{from_js, install, install_with_transport, to_js};

// ---------------------------------------------------------------- the fake host

#[derive(Clone, Default)]
struct Shape {
    id: u64,
    props: Map<String, Json>,
    text: String,
    frame: Map<String, Json>,
    font: Map<String, Json>,
    fill: Map<String, Json>,
    line: Map<String, Json>,
}

#[derive(Clone, Default)]
struct Slide {
    id: u64,
    shapes: Vec<Shape>,
}

#[derive(Clone, Default)]
struct Deck {
    next: u64,
    slides: Vec<Slide>,
    selected_slides: Vec<u64>,
    selected_shapes: Vec<u64>,
    settings: Json,
}

impl Deck {
    fn new() -> Deck {
        Deck { next: 258, slides: vec![Slide { id: 256, shapes: Vec::new() }], settings: json!({}), ..Deck::default() }
    }
    fn id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

thread_local! {
    static DECK: RefCell<Deck> = RefCell::new(Deck::new());
    static LOG: RefCell<Vec<Json>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug)]
enum T {
    Pres,
    Slides { selected: bool },
    Slide(usize),
    Layout,
    Master,
    Shapes(Option<usize>),
    Shape(usize, usize),
    Frame(usize, usize),
    Range(usize, usize),
    Font(usize, usize),
    Fill(usize, usize),
    Line(usize, usize),
    Document,
    Settings,
}

type Fail = (&'static str, String);

fn not_found(what: impl std::fmt::Display) -> Fail {
    ("ItemNotFound", format!("{what} doesn't exist"))
}

/// An argument, with `{"result": i}` replaced by the id action `i` answered.
fn arg(a: &Json, results: &[Json]) -> Json {
    match a.get("result").and_then(Json::as_u64) {
        Some(i) => results.get(i as usize).and_then(|r| r.get("id")).cloned().unwrap_or(Json::Null),
        None => a.clone(),
    }
}

fn find_shape(d: &Deck, id: u64) -> Option<(usize, usize)> {
    d.slides.iter().enumerate().find_map(|(s, sl)| sl.shapes.iter().position(|x| x.id == id).map(|i| (s, i)))
}

fn resolve(d: &Deck, path: &[Json], results: &[Json]) -> Result<T, Fail> {
    let mut t: Option<T> = None;
    for step in path {
        let p = step.get("p").and_then(Json::as_str);
        let m = step.get("m").and_then(Json::as_str);
        let a: Vec<Json> = step.get("a").and_then(Json::as_array).map(|v| v.iter().map(|x| arg(x, results)).collect()).unwrap_or_default();
        let key = a.first().and_then(|k| k.as_str().map(str::to_string).or_else(|| k.as_u64().map(|n| n.to_string()))).unwrap_or_default();
        let at = a.first().and_then(Json::as_u64).unwrap_or(u64::MAX) as usize;
        t = Some(match (t, p, m) {
            (None, Some("presentation"), _) => T::Pres,
            (None, Some("document"), _) => T::Document,
            (None, Some("settings"), _) => T::Settings,
            (Some(T::Pres), Some("slides"), _) => T::Slides { selected: false },
            (Some(T::Pres), _, Some("getSelectedSlides")) => T::Slides { selected: true },
            (Some(T::Pres), _, Some("getSelectedShapes")) => T::Shapes(None),
            (Some(T::Slides { selected }), _, Some("getItem")) => {
                let i = d.slides.iter().position(|s| s.id.to_string() == key && (!selected || d.selected_slides.contains(&s.id)));
                T::Slide(i.ok_or_else(|| not_found(format!("slide {key}")))?)
            }
            (Some(T::Slides { selected: false }), _, Some("getItemAt")) => {
                if at >= d.slides.len() {
                    return Err(not_found(format!("slide #{at}")));
                }
                T::Slide(at)
            }
            (Some(T::Slide(s)), Some("shapes"), _) => T::Shapes(Some(s)),
            (Some(T::Slide(_)), Some("layout"), _) => T::Layout,
            (Some(T::Slide(_)), Some("slideMaster"), _) => T::Master,
            (Some(T::Shapes(Some(s))), _, Some("getItem")) => {
                let i = d.slides[s].shapes.iter().position(|x| x.id.to_string() == key);
                T::Shape(s, i.ok_or_else(|| not_found(format!("shape {key}")))?)
            }
            (Some(T::Shapes(None)), _, Some("getItem")) => {
                let id = key.parse::<u64>().ok().filter(|id| d.selected_shapes.contains(id));
                let (s, i) = id.and_then(|id| find_shape(d, id)).ok_or_else(|| not_found(format!("shape {key}")))?;
                T::Shape(s, i)
            }
            (Some(T::Shapes(Some(s))), _, Some("getItemAt")) => {
                if at >= d.slides[s].shapes.len() {
                    return Err(not_found(format!("shape #{at}")));
                }
                T::Shape(s, at)
            }
            (Some(T::Shape(s, i)), Some("textFrame"), _) => T::Frame(s, i),
            (Some(T::Shape(s, i)), Some("fill"), _) => T::Fill(s, i),
            (Some(T::Shape(s, i)), Some("lineFormat"), _) => T::Line(s, i),
            (Some(T::Frame(s, i)), Some("textRange"), _) => T::Range(s, i),
            (Some(T::Range(s, i)), Some("font"), _) => T::Font(s, i),
            (t, _, _) => return Err(("InvalidArgument", format!("no step {step} from {t:?}"))),
        });
    }
    t.ok_or_else(|| ("InvalidArgument", "empty path".into()))
}

fn shape_props(sh: &Shape) -> Map<String, Json> {
    let mut m = sh.props.clone();
    m.insert("id".into(), json!(sh.id.to_string()));
    m
}

fn pick(all: &Map<String, Json>, props: &[String]) -> Json {
    Json::Object(props.iter().filter_map(|p| all.get(p).map(|v| (p.clone(), v.clone()))).collect())
}

fn obj_of(pairs: &[(&str, Json)]) -> Map<String, Json> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn load(d: &Deck, t: T, props: &[String]) -> Result<Json, Fail> {
    let items = props.iter().any(|p| p == "items");
    let rest: Vec<String> = props.iter().filter(|p| *p != "items").cloned().collect();
    let list: Option<Vec<Map<String, Json>>> = match t {
        T::Slides { selected } => Some(
            d.slides
                .iter()
                .enumerate()
                .filter(|(_, s)| !selected || d.selected_slides.contains(&s.id))
                .map(|(i, s)| obj_of(&[("id", json!(s.id.to_string())), ("index", json!(i))]))
                .collect(),
        ),
        T::Shapes(Some(s)) => Some(d.slides[s].shapes.iter().map(shape_props).collect()),
        T::Shapes(None) => {
            Some(d.selected_shapes.iter().filter_map(|id| find_shape(d, *id)).map(|(s, i)| shape_props(&d.slides[s].shapes[i])).collect())
        }
        _ => None,
    };
    if let Some(list) = list {
        if items {
            let mut want = rest.clone();
            want.insert(0, "id".into());
            return Ok(json!({"items": list.iter().map(|m| pick(m, &want)).collect::<Vec<_>>()}));
        }
        return Ok(json!({"count": list.len()}));
    }
    let all: Map<String, Json> = match t {
        T::Pres => obj_of(&[("title", json!("Deck"))]),
        T::Slide(s) => obj_of(&[("id", json!(d.slides[s].id.to_string())), ("index", json!(s))]),
        T::Layout => obj_of(&[("id", json!("2147483649")), ("name", json!("Title and Content"))]),
        T::Master => obj_of(&[("id", json!("2147483648")), ("name", json!("Office Theme"))]),
        T::Shape(s, i) => shape_props(&d.slides[s].shapes[i]),
        T::Frame(s, i) => {
            let sh = &d.slides[s].shapes[i];
            let mut m = sh.frame.clone();
            m.insert("hasText".into(), json!(!sh.text.is_empty()));
            m
        }
        T::Range(s, i) => {
            let text = d.slides[s].shapes[i].text.clone();
            obj_of(&[("length", json!(text.chars().count())), ("start", json!(0)), ("text", json!(text))])
        }
        T::Font(s, i) => d.slides[s].shapes[i].font.clone(),
        T::Fill(s, i) => d.slides[s].shapes[i].fill.clone(),
        T::Line(s, i) => d.slides[s].shapes[i].line.clone(),
        T::Settings => return Ok(d.settings.clone()),
        _ => return Err(("InvalidArgument", format!("can't load {t:?}"))),
    };
    Ok(if rest.is_empty() { Json::Object(all) } else { pick(&all, &rest) })
}

fn set(d: &mut Deck, t: T, name: &str, v: Json) -> Result<(), Fail> {
    let map = match t {
        T::Shape(s, i) if ["name", "left", "top", "width", "height"].contains(&name) => &mut d.slides[s].shapes[i].props,
        T::Frame(s, i) if ["wordWrap", "autoSizeSetting"].contains(&name) => &mut d.slides[s].shapes[i].frame,
        T::Range(s, i) if name == "text" => {
            d.slides[s].shapes[i].text = v.as_str().unwrap_or_default().to_string();
            return Ok(());
        }
        T::Font(s, i) if ["bold", "italic", "underline", "size", "name", "color"].contains(&name) => &mut d.slides[s].shapes[i].font,
        T::Fill(s, i) if ["foregroundColor", "transparency"].contains(&name) => {
            d.slides[s].shapes[i].fill.insert("type".into(), json!("solid"));
            &mut d.slides[s].shapes[i].fill
        }
        T::Line(s, i) if ["color", "weight", "visible", "dashStyle"].contains(&name) => &mut d.slides[s].shapes[i].line,
        _ => return Err(("InvalidArgument", format!("can't set {name} of {t:?}"))),
    };
    map.insert(name.into(), v);
    Ok(())
}

fn add_shape(d: &mut Deck, s: usize, kind: &str, args: &[Json]) -> Json {
    let id = d.id();
    let opts = args.get(1).cloned().unwrap_or(json!({}));
    let n = d.slides[s].shapes.len() + 1;
    let name = match kind {
        "textBox" => format!("TextBox {n}"),
        "line" => format!("Straight Connector {n}"),
        _ => format!("{} {n}", args.first().and_then(Json::as_str).unwrap_or("Shape")),
    };
    let mut props = obj_of(&[("name", json!(name)), ("type", json!(kind))]);
    for k in ["left", "top", "width", "height"] {
        props.insert(k.into(), opts.get(k).cloned().unwrap_or(json!(100)));
    }
    let text = if kind == "textBox" { args.first().and_then(Json::as_str).unwrap_or_default().to_string() } else { String::new() };
    let font = obj_of(&[
        ("bold", json!(false)),
        ("italic", json!(false)),
        ("size", json!(18)),
        ("name", json!("Calibri")),
        ("color", json!("#000000")),
        ("underline", json!("none")),
    ]);
    let fill = obj_of(&[("type", json!(if kind == "textBox" { "noFill" } else { "solid" }))]);
    d.slides[s].shapes.push(Shape { id, props, text, font, fill, ..Shape::default() });
    if kind == "geometricShape" {
        LOG.with(|l| l.borrow_mut().push(json!({"geometry": args.first()})));
    }
    json!({"id": id.to_string()})
}

fn call(d: &mut Deck, t: T, name: &str, args: &[Json]) -> Result<Json, Fail> {
    match (t, name) {
        (T::Slides { selected: false }, "add") => {
            let id = d.id();
            let at = args.first().and_then(|o| o.get("index")).and_then(Json::as_u64).map(|i| i as usize).unwrap_or(d.slides.len());
            d.slides.insert(at.min(d.slides.len()), Slide { id, shapes: Vec::new() });
            Ok(json!({"id": id.to_string()}))
        }
        (T::Slide(s), "delete") => {
            d.slides.remove(s);
            Ok(Json::Null)
        }
        (T::Shapes(Some(s)), "addTextBox") => Ok(add_shape(d, s, "textBox", args)),
        (T::Shapes(Some(s)), "addGeometricShape") => Ok(add_shape(d, s, "geometricShape", args)),
        (T::Shapes(Some(s)), "addLine") => Ok(add_shape(d, s, "line", args)),
        (T::Shape(s, i), "delete") => {
            d.slides[s].shapes.remove(i);
            Ok(Json::Null)
        }
        (T::Fill(s, i), "setSolidColor") => {
            let f = &mut d.slides[s].shapes[i].fill;
            f.insert("type".into(), json!("solid"));
            f.insert("foregroundColor".into(), json!(args.first().and_then(Json::as_str).unwrap_or_default().to_uppercase()));
            Ok(Json::Null)
        }
        (T::Fill(s, i), "clear") => {
            d.slides[s].shapes[i].fill = obj_of(&[("type", json!("noFill"))]);
            Ok(Json::Null)
        }
        (T::Pres, "setSelectedSlides") | (T::Slide(_), "setSelectedShapes") => {
            let ids: Vec<u64> = args.first().and_then(Json::as_array).into_iter().flatten().filter_map(|v| v.as_str()?.parse().ok()).collect();
            if let T::Slide(s) = t {
                if let Some(bad) = ids.iter().find(|id| !d.slides[s].shapes.iter().any(|x| x.id == **id)) {
                    return Err(not_found(format!("shape {bad}")));
                }
                d.selected_shapes = ids;
            } else {
                if let Some(bad) = ids.iter().find(|id| !d.slides.iter().any(|x| x.id == **id)) {
                    return Err(not_found(format!("slide {bad}")));
                }
                d.selected_slides = ids;
            }
            Ok(Json::Null)
        }
        (T::Document, "getSelectedData") if args.first().and_then(Json::as_str) == Some("slideRange") => {
            let slides: Vec<Json> = d
                .slides
                .iter()
                .enumerate()
                .filter(|(_, s)| d.selected_slides.contains(&s.id))
                .map(|(i, s)| json!({"id": s.id, "title": "", "index": i + 1}))
                .collect();
            Ok(json!({"slides": slides}))
        }
        (T::Document, "setSelectedData") if args.get(1).is_some() => Ok(Json::Null),
        (T::Document, "goToById") => Ok(Json::Null),
        (T::Document, "getSelectedData") => {
            let (s, i) = d.selected_shapes.first().and_then(|id| find_shape(d, *id)).ok_or(("InvalidArgument", "nothing selected".to_string()))?;
            Ok(json!(d.slides[s].shapes[i].text))
        }
        (T::Document, "setSelectedData") => {
            let (s, i) = d.selected_shapes.first().and_then(|id| find_shape(d, *id)).ok_or(("InvalidArgument", "nothing selected".to_string()))?;
            d.slides[s].shapes[i].text = args.first().and_then(Json::as_str).unwrap_or_default().to_string();
            Ok(Json::Null)
        }
        (T::Settings, "save") => {
            d.settings = args.first().cloned().unwrap_or(json!({}));
            Ok(Json::Null)
        }
        _ => Err(("InvalidArgument", format!("no method {name} on {t:?}"))),
    }
}

/// `office.sync`: every action in order, all or nothing.
fn office_sync(request: &str) -> String {
    let req: Json = serde_json::from_str(request).unwrap();
    let actions = req["actions"].as_array().cloned().unwrap_or_default();
    LOG.with(|l| l.borrow_mut().extend(actions.iter().cloned()));
    let mut out = DECK.with(|cell| {
        let mut deck = cell.borrow().clone();
        let mut results: Vec<Json> = Vec::new();
        for (i, a) in actions.iter().enumerate() {
            let path = a["path"].as_array().cloned().unwrap_or_default();
            let r = resolve(&deck, &path, &results).and_then(|t| match a["op"].as_str() {
                Some("load") => {
                    let props: Vec<String> = a["props"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
                    load(&deck, t, &props)
                }
                Some("set") => set(&mut deck, t, a["prop"].as_str().unwrap_or_default(), a["value"].clone()).map(|_| Json::Null),
                Some("call") => call(&mut deck, t, a["method"].as_str().unwrap_or_default(), a["args"].as_array().map(Vec::as_slice).unwrap_or(&[])),
                _ => Err(("InvalidArgument", "bad op".into())),
            });
            match r {
                Ok(v) => results.push(v),
                Err((code, message)) => return json!({"error": {"code": code, "message": message, "action": i}}),
            }
        }
        *cell.borrow_mut() = deck;
        json!({"results": results})
    });
    out["id"] = req["id"].clone();
    out.to_string()
}

/// A fresh deck and the library installed with the fake host as its transport.
fn setup() {
    DECK.with(|d| *d.borrow_mut() = Deck::new());
    LOG.with(|l| l.borrow_mut().clear());
    install_with_transport(|r| office_sync(&r));
}

fn deck<R>(f: impl FnOnce(&Deck) -> R) -> R {
    DECK.with(|d| f(&d.borrow()))
}

fn log() -> Vec<Json> {
    LOG.with(|l| l.borrow().clone())
}

// ---------------------------------------------------------------- helpers

fn global(name: &str) -> JsValue {
    Reflect::get(&js_sys::global(), &name.into()).unwrap()
}

fn g(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &k.into()).unwrap()
}

fn path(o: &JsValue, keys: &[&str]) -> JsValue {
    keys.iter().fold(o.clone(), |o, k| g(&o, k))
}

fn set_js(o: &JsValue, k: &str, v: Json) {
    assert!(Reflect::set(o, &k.into(), &to_js(&v)).unwrap(), "{k} is settable");
}

fn call_js(o: &JsValue, name: &str, args: &[JsValue]) -> JsValue {
    let f: Function = g(o, name).dyn_into().unwrap_or_else(|_| panic!("{name} is a function"));
    f.apply(o, &args.iter().cloned().collect::<Array>()).unwrap()
}

fn s(v: &str) -> JsValue {
    v.into()
}

fn code(e: &JsValue) -> String {
    g(e, "code").as_string().unwrap_or_default()
}

/// `PowerPoint.run(batch)` with an async Rust batch; resolves with what the batch returns.
async fn pp_run<F>(batch: impl FnOnce(JsValue) -> F + 'static) -> Result<JsValue, JsValue>
where
    F: std::future::Future<Output = Result<JsValue, JsValue>> + 'static,
{
    let batch = Closure::once_into_js(move |ctx: JsValue| -> Promise { future_to_promise(batch(ctx)) });
    let run = g(&global("PowerPoint"), "run");
    let p: Promise = run.dyn_into::<Function>().unwrap().call1(&JsValue::UNDEFINED, &batch).unwrap().dyn_into().unwrap();
    JsFuture::from(p).await
}

async fn sync(ctx: &JsValue) -> Result<JsValue, JsValue> {
    JsFuture::from(call_js(ctx, "sync", &[]).dyn_into::<Promise>().unwrap()).await
}

fn slides(ctx: &JsValue) -> JsValue {
    path(ctx, &["presentation", "slides"])
}

// ---------------------------------------------------------------- tests

#[wasm_bindgen_test]
async fn a_slide_and_a_text_box_are_added_written_listed_and_deleted() {
    setup();
    let out = pp_run(|ctx| async move {
        let slides = slides(&ctx);
        call_js(&slides, "add", &[]);
        let count = call_js(&slides, "getCount", &[]);
        assert_eq!(code(&Reflect::get(&count, &"value".into()).unwrap_err()), "ValueNotLoaded");
        sync(&ctx).await?;
        assert_eq!(g(&count, "value").as_f64(), Some(2.0));
        let shapes = g(&call_js(&slides, "getItemAt", &[JsValue::from(1)]), "shapes");
        let opts = to_js(&json!({"left": 50, "top": 60, "width": 300, "height": 40}));
        let tb = call_js(&shapes, "addTextBox", &[s("Hello"), opts]);
        let range = path(&tb, &["textFrame", "textRange"]);
        set_js(&range, "text", json!("Hi there"));
        set_js(&g(&range, "font"), "bold", json!(true));
        call_js(&g(&tb, "fill"), "setSolidColor", &[s("#ff0000")]);
        call_js(&tb, "load", &[s("name, type, left")]);
        // Not loaded until the sync: reading it throws PropertyNotLoaded.
        assert_eq!(code(&Reflect::get(&tb, &"name".into()).unwrap_err()), "PropertyNotLoaded");
        assert_eq!(from_js(&g(&range, "text")), json!("Hi there"), "a value just set is known at once");
        sync(&ctx).await?;
        call_js(&shapes, "load", &[s("items/name")]);
        let n = call_js(&shapes, "getCount", &[]);
        call_js(&range, "load", &[s("text, length")]);
        call_js(&g(&range, "font"), "load", &[]);
        call_js(&g(&tb, "fill"), "load", &[s("type, foregroundColor")]);
        sync(&ctx).await?;
        let items: Array = g(&shapes, "items").dyn_into()?;
        let names: Vec<Json> = items.iter().map(|i| from_js(&g(&i, "name"))).collect();
        let mut result = json!({
            "id": from_js(&g(&tb, "id")), "name": from_js(&g(&tb, "name")), "type": from_js(&g(&tb, "type")), "left": from_js(&g(&tb, "left")),
            "names": names, "count": from_js(&g(&n, "value")), "text": from_js(&g(&range, "text")), "length": from_js(&g(&range, "length")),
            "bold": from_js(&path(&range, &["font", "bold"])), "underline": from_js(&path(&range, &["font", "underline"])),
            "fill": from_js(&call_js(&g(&tb, "fill"), "toJSON", &[])), "json": from_js(&call_js(&shapes, "toJSON", &[])),
            "itemId": from_js(&g(&items.get(0), "id")),
        });
        call_js(&tb, "delete", &[]);
        let after = call_js(&shapes, "getCount", &[]);
        sync(&ctx).await?;
        result["after"] = from_js(&g(&after, "value"));
        Ok(to_js(&result))
    })
    .await
    .unwrap();
    let out = from_js(&out);
    assert_eq!(out["id"], "260", "an added object's id is known after the sync");
    assert_eq!((out["name"].as_str(), out["type"].as_str(), out["left"].as_i64()), (Some("TextBox 1"), Some("TextBox"), Some(50)));
    assert_eq!(out["names"], json!(["TextBox 1"]));
    assert_eq!(out["itemId"], "260");
    assert_eq!((out["count"].as_i64(), out["after"].as_i64()), (Some(1), Some(0)));
    assert_eq!((out["text"].as_str(), out["length"].as_i64()), (Some("Hi there"), Some(8)));
    assert_eq!((out["bold"].as_bool(), out["underline"].as_str()), (Some(true), Some("None")));
    assert_eq!(out["fill"], json!({"type": "Solid", "foregroundColor": "#FF0000"}));
    assert_eq!(out["json"], json!({"items": [{"id": "260", "name": "TextBox 1"}]}));
    // Within the batch that added it, the text box was named by the add's result.
    let wire = log();
    let add = wire.iter().position(|a| a["method"] == "addTextBox").unwrap();
    // The second sync starts with the add: its action 0.
    assert_eq!(wire[add - 1]["props"], json!(["count"]));
    assert_eq!(wire[add]["args"], json!(["Hello", {"left": 50, "top": 60, "width": 300, "height": 40}]));
    assert_eq!(wire[add + 1]["path"][4], json!({"m": "getItem", "a": [{"result": 0}]}));
    let del = wire.iter().find(|a| a["method"] == "delete").unwrap();
    assert_eq!(del["path"][4], json!({"m": "getItem", "a": ["260"]}));
    assert_eq!(deck(|d| (d.slides.len(), d.slides[1].shapes.len())), (2, 0));
}

#[wasm_bindgen_test]
async fn a_failing_sync_rejects_with_the_code_and_changes_nothing() {
    setup();
    let e = pp_run(|ctx| async move {
        call_js(&slides(&ctx), "add", &[]);
        let missing = call_js(&slides(&ctx), "getItem", &[s("999")]);
        call_js(&g(&missing, "shapes"), "load", &[s("items")]);
        sync(&ctx).await
    })
    .await
    .unwrap_err();
    assert_eq!(code(&e), "ItemNotFound");
    assert_eq!(g(&e, "name").as_string().as_deref(), Some("RichApi.Error"));
    assert!(e.is_instance_of::<js_sys::Error>(), "OfficeExtension.Error is an Error");
    assert_eq!(from_js(&g(&e, "debugInfo")), json!({"code": "ItemNotFound", "message": "slide 999 doesn't exist", "errorLocation": 1}));
    assert_eq!(deck(|d| d.slides.len()), 1, "the add in the same sync was taken back");
    // PowerPoint.run without a sync still sends what was queued.
    pp_run(|ctx| async move {
        call_js(&slides(&ctx), "add", &[to_js(&json!({"index": 0, "layoutId": "x"}))]);
        Ok(JsValue::UNDEFINED)
    })
    .await
    .unwrap();
    assert_eq!(deck(|d| d.slides.iter().map(|s| s.id).collect::<Vec<_>>()), [259, 256]);
    assert!(log().iter().any(|a| a["method"] == "add" && a["args"] == json!([{"index": 0, "layoutId": "x"}])));
}

#[wasm_bindgen_test]
async fn shapes_take_powerpoint_enums_and_answer_them() {
    setup();
    let out = pp_run(|ctx| async move {
        let pp = global("PowerPoint");
        let star = path(&pp, &["GeometricShapeType", "star5"]);
        assert_eq!(star.as_string().as_deref(), Some("Star5"));
        assert_eq!(path(&pp, &["ShapeType", "geometricShape"]).as_string().as_deref(), Some("GeometricShape"));
        let shapes = g(&call_js(&slides(&ctx), "getItemAt", &[JsValue::from(0)]), "shapes");
        let shape = call_js(&shapes, "addGeometricShape", &[star, to_js(&json!({"left": 10}))]);
        let line = call_js(&shapes, "addLine", &[]);
        set_js(&g(&line, "lineFormat"), "dashStyle", json!("RoundDot"));
        set_js(&g(&shape, "fill"), "transparency", json!(0.5));
        call_js(&shape, "load", &[]);
        call_js(&g(&line, "lineFormat"), "load", &[s("dashStyle")]);
        let bad = Reflect::apply(&g(&shapes, "addGeometricShape").dyn_into::<Function>()?, &shapes, &Array::new()).unwrap_err();
        assert_eq!(code(&bad), "InvalidArgument");
        sync(&ctx).await?;
        Ok(to_js(&json!({
            "shape": from_js(&call_js(&shape, "toJSON", &[])),
            "dash": from_js(&path(&line, &["lineFormat", "dashStyle"])),
            "lineType": deck(|d| d.slides[0].shapes[1].props["type"].clone()),
        })))
    })
    .await
    .unwrap();
    let out = from_js(&out);
    assert_eq!(out["shape"], json!({"id": "259", "name": "star5 1", "type": "GeometricShape", "left": 10, "top": 100, "width": 100, "height": 100}));
    assert_eq!(out["dash"], "RoundDot");
    assert_eq!(out["lineType"], "line");
    assert!(log().iter().any(|a| a == &json!({"geometry": "star5"})), "enum values travel camelCase");
    assert!(log().iter().any(|a| a["method"] == "addLine" && a["args"][0] == "straight"));
    assert!(log().iter().any(|a| a["prop"] == "dashStyle" && a["value"] == "roundDot"));
}

#[wasm_bindgen_test]
async fn selection_layouts_masters_and_selected_text() {
    setup();
    let out = pp_run(|ctx| async move {
        let pres = g(&ctx, "presentation");
        call_js(&slides(&ctx), "add", &[]);
        let first = call_js(&slides(&ctx), "getItemAt", &[JsValue::from(0)]);
        let tb = call_js(&g(&first, "shapes"), "addTextBox", &[s("Selected text")]);
        call_js(&tb, "load", &[s("id")]);
        sync(&ctx).await?;
        let id = g(&tb, "id");
        call_js(&first, "setSelectedShapes", &[Array::of1(&id).into()]);
        call_js(&pres, "setSelectedSlides", &[to_js(&json!(["259"]))]);
        let sel = call_js(&pres, "getSelectedSlides", &[]);
        call_js(&sel, "load", &[s("items")]);
        let shapes = call_js(&pres, "getSelectedShapes", &[]);
        call_js(&shapes, "load", &[s("items/name")]);
        let layout = g(&first, "layout");
        call_js(&layout, "load", &[s("id, name")]);
        let master = g(&first, "slideMaster");
        call_js(&master, "load", &[]);
        call_js(&pres, "load", &[s("title")]);
        sync(&ctx).await?;
        let sel_items: Array = g(&sel, "items").dyn_into()?;
        let shape_items: Array = g(&shapes, "items").dyn_into()?;
        Ok(to_js(&json!({
            "slides": sel_items.iter().map(|i| from_js(&g(&i, "id"))).collect::<Vec<_>>(),
            "shapes": shape_items.iter().map(|i| from_js(&g(&i, "name"))).collect::<Vec<_>>(),
            "layout": from_js(&call_js(&layout, "toJSON", &[])), "master": from_js(&g(&master, "name")), "title": from_js(&g(&pres, "title")),
        })))
    })
    .await
    .unwrap();
    let out = from_js(&out);
    assert_eq!(out["slides"], json!(["259"]));
    assert_eq!(out["shapes"], json!(["TextBox 1"]));
    assert_eq!(out["layout"], json!({"id": "2147483649", "name": "Title and Content"}));
    assert_eq!((out["master"].as_str(), out["title"].as_str()), (Some("Office Theme"), Some("Deck")));
    // The common API reads and writes the selected text box's text.
    let text = document_async("getSelectedDataAsync", s("text")).await;
    assert_eq!(g(&text, "status").as_string().as_deref(), Some("succeeded"));
    assert_eq!(g(&text, "value").as_string().as_deref(), Some("Selected text"));
    let set_r = document_async("setSelectedDataAsync", s("Replaced")).await;
    assert_eq!(g(&set_r, "status").as_string().as_deref(), Some("succeeded"));
    assert_eq!(deck(|d| d.slides[0].shapes[0].text.clone()), "Replaced");
    let html = document_async("getSelectedDataAsync", s("html")).await;
    assert_eq!(g(&html, "status").as_string().as_deref(), Some("failed"));
    assert_eq!(path(&html, &["error", "code"]).as_string().as_deref(), Some("NotSupported"));
}

/// Calls `Office.context.document.<name>(first, callback)` and waits for the callback's result.
async fn document_async(name: &str, first: JsValue) -> JsValue {
    let doc = path(&global("Office"), &["context", "document"]);
    let name = name.to_string();
    let p = Promise::new(&mut |resolve, _| {
        let _ = call_js(&doc, &name, &[first.clone(), resolve.into()]);
    });
    JsFuture::from(p).await.unwrap()
}

async fn callback_only(o: &JsValue, name: &str) -> JsValue {
    let o = o.clone();
    let name = name.to_string();
    let p = Promise::new(&mut |resolve, _| {
        let _ = call_js(&o, &name, &[resolve.into()]);
    });
    JsFuture::from(p).await.unwrap()
}

#[wasm_bindgen_test]
async fn settings_requirements_and_the_globals() {
    setup();
    let office = global("Office");
    let info = JsFuture::from(call_js(&office, "onReady", &[]).dyn_into::<Promise>().unwrap()).await.unwrap();
    assert_eq!(g(&info, "host").as_string().as_deref(), Some("PowerPoint"));
    assert_eq!(path(&office, &["HostType", "PowerPoint"]).as_string().as_deref(), Some("PowerPoint"));
    assert_eq!(path(&office, &["context", "host"]).as_string().as_deref(), Some("PowerPoint"));
    let settings = path(&office, &["context", "document", "settings"]);
    call_js(&settings, "set", &[s("theme"), s("dark")]);
    assert_eq!(call_js(&settings, "get", &[s("theme")]).as_string().as_deref(), Some("dark"));
    let saved = callback_only(&settings, "saveAsync").await;
    assert_eq!(g(&saved, "status").as_string().as_deref(), Some("succeeded"));
    // Installed again (the pane reloaded), the settings come back from the host.
    install_with_transport(|r| office_sync(&r));
    let _ = JsFuture::from(call_js(&office, "onReady", &[]).dyn_into::<Promise>().unwrap()).await.unwrap();
    assert_eq!(call_js(&settings, "get", &[s("theme")]).as_string().as_deref(), Some("dark"));
    assert!(call_js(&settings, "get", &[s("missing")]).is_null());
    let req = path(&office, &["context", "requirements"]);
    let supported = |name: &str, v: &str| call_js(&req, "isSetSupported", &[s(name), s(v)]).as_bool();
    assert_eq!(supported("PowerPointApi", "1.4"), Some(true));
    assert_eq!(supported("PowerPointApi", "1.5"), Some(false));
    assert_eq!(supported("ExcelApi", "1.1"), Some(false));
    // The globals can't be replaced by a later script (Microsoft's office.js, say); no Excel.
    let _ = Reflect::set(&js_sys::global(), &"PowerPoint".into(), &JsValue::from(1));
    assert!(global("PowerPoint").is_object());
    assert!(global("Excel").is_undefined());
    let created = JsFuture::from(call_js(&global("PowerPoint"), "createPresentation", &[]).dyn_into::<Promise>().unwrap()).await.unwrap_err();
    assert_eq!(code(&created), "NotImplemented");
}

#[wasm_bindgen_test]
async fn the_page_transport_posts_and_takes_replies() {
    DECK.with(|d| *d.borrow_mut() = Deck::new());
    // What the desktop web view provides: `__craftOffice.post`, answered later through
    // `__craftOfficeReply`.
    let post = Closure::<dyn Fn(String)>::new(|request: String| {
        spawn_local(async move {
            let answer = office_sync(&request);
            let reply: Function = global("__craftOfficeReply").dyn_into().unwrap();
            reply.call1(&JsValue::UNDEFINED, &answer.into()).unwrap();
        });
    });
    let host = js_sys::Object::new();
    Reflect::set(&host, &"post".into(), &post.into_js_value()).unwrap();
    Reflect::set(&js_sys::global(), &"__craftOffice".into(), &host).unwrap();
    install();
    let n = pp_run(|ctx| async move {
        call_js(&slides(&ctx), "add", &[]);
        let n = call_js(&slides(&ctx), "getCount", &[]);
        sync(&ctx).await?;
        Ok(g(&n, "value"))
    })
    .await
    .unwrap();
    assert_eq!(n.as_f64(), Some(2.0));
    let _ = Reflect::delete_property(&js_sys::global(), &"__craftOffice".into());
}

// ---------------------------------------------------------------- the common API's newer parts

thread_local! {
    /// What the page posted that isn't a request (`{"event": …}`).
    static POSTED: RefCell<Vec<Json>> = const { RefCell::new(Vec::new()) };
}

/// A host channel that only records the page's events (requests go through the test transport).
fn record_events() {
    POSTED.with(|p| p.borrow_mut().clear());
    let post = Closure::<dyn Fn(String)>::new(|m: String| {
        let m: Json = serde_json::from_str(&m).unwrap();
        if m.get("event").is_some() {
            POSTED.with(|p| p.borrow_mut().push(m));
        }
    });
    let host = js_sys::Object::new();
    Reflect::set(&host, &"post".into(), &post.into_js_value()).unwrap();
    Reflect::set(&js_sys::global(), &"__craftOffice".into(), &host).unwrap();
}

fn stop_recording() {
    let _ = Reflect::delete_property(&js_sys::global(), &"__craftOffice".into());
}

fn posted() -> Vec<Json> {
    POSTED.with(|p| p.borrow().clone())
}

/// Lets the queued tasks run (a `setTimeout(0)`).
async fn tick() {
    let p = Promise::new(&mut |resolve, _| {
        let set_timeout: Function = global("setTimeout").dyn_into().unwrap();
        set_timeout.call2(&JsValue::UNDEFINED, &resolve, &JsValue::from(0)).unwrap();
    });
    JsFuture::from(p).await.unwrap();
}

async fn ready() {
    JsFuture::from(call_js(&global("Office"), "onReady", &[]).dyn_into::<Promise>().unwrap()).await.unwrap();
}

/// A JS function that records its first argument under `name`.
fn recorder(name: &'static str) -> JsValue {
    Closure::<dyn Fn(JsValue)>::new(move |arg: JsValue| {
        LOG.with(|l| l.borrow_mut().push(json!({"called": name, "arg": from_js(&arg)})));
    })
    .into_js_value()
}

fn calls(name: &str) -> Vec<Json> {
    log().into_iter().filter(|e| e["called"] == name).map(|e| e["arg"].clone()).collect()
}

#[wasm_bindgen_test]
async fn enums_request_context_and_set() {
    setup();
    ready().await;
    let office = global("Office");
    for (e, k, v) in [
        ("EventType", "DocumentSelectionChanged", "documentSelectionChanged"),
        ("Index", "First", "first"),
        ("GoToType", "Slide", "slide"),
        ("GoToType", "Index", "index"),
        ("CoercionType", "Image", "image"),
        ("CoercionType", "SlideRange", "slideRange"),
        ("HostType", "PowerPoint", "PowerPoint"),
        ("InitializationReason", "Inserted", "inserted"),
    ] {
        assert_eq!(path(&office, &[e, k]).as_string().as_deref(), Some(v), "Office.{e}.{k}");
    }
    // Office's members can't be swapped by a later script.
    let _ = Reflect::set(&office, &"onReady".into(), &JsValue::from(1));
    assert!(g(&office, "onReady").is_function());
    let _ = Reflect::set(&global("PowerPoint"), &"run".into(), &JsValue::from(1));
    assert!(g(&global("PowerPoint"), "run").is_function());
    // `new PowerPoint.RequestContext()` and an explicit sync.
    let ctor = g(&global("PowerPoint"), "RequestContext").unchecked_into::<Function>();
    let ctx = Reflect::construct(&ctor, &Array::new()).unwrap();
    call_js(&slides(&ctx), "add", &[]);
    let n = call_js(&slides(&ctx), "getCount", &[]);
    sync(&ctx).await.unwrap();
    assert_eq!(g(&n, "value").as_f64(), Some(2.0));
    // `set({...})` follows navigation properties.
    let out = pp_run(|ctx| async move {
        let slide = call_js(&slides(&ctx), "getItemAt", &[JsValue::from(0)]);
        let shape = call_js(&g(&slide, "shapes"), "addTextBox", &[s("Hi")]);
        call_js(
            &shape,
            "set",
            &[to_js(
                &json!({"name": "Named", "left": 40, "fill": {"foregroundColor": "#FF0000"}, "textFrame": {"textRange": {"font": {"bold": true}}}}),
            )],
        );
        let set: Function = g(&shape, "set").dyn_into().unwrap();
        assert!(set.call1(&shape, &JsValue::from(3)).is_err(), "set() takes an object");
        call_js(&shape, "load", &[s("name, left")]);
        sync(&ctx).await?;
        Ok(to_js(&json!([g(&shape, "name").as_string(), g(&shape, "left").as_f64()])))
    })
    .await
    .unwrap();
    assert_eq!(from_js(&out), json!(["Named", 40]));
    let sh = deck(|d| d.slides[0].shapes[0].clone());
    assert_eq!(sh.fill.get("foregroundColor"), Some(&json!("#FF0000")));
    assert_eq!(sh.font.get("bold"), Some(&json!(true)));
}

#[wasm_bindgen_test]
async fn initialize_set_late_is_still_called_and_handlers_answer() {
    setup();
    record_events();
    ready().await;
    assert!(posted().contains(&json!({"event": "ready"})), "the page says it's ready: {:?}", posted());
    let office = global("Office");
    Reflect::set(&office, &"initialize".into(), &recorder("initialize")).unwrap();
    tick().await;
    assert_eq!(calls("initialize"), vec![json!("inserted")]);
    assert!(g(&office, "initialize").is_function());
    // An event DeckCraft doesn't send fails through the callback, asynchronously; no TypeError.
    let doc = path(&office, &["context", "document"]);
    let failed = Promise::new(&mut |resolve, _| {
        call_js(&doc, "addHandlerAsync", &[s("bindingDataChanged"), recorder("never"), resolve.into()]);
    });
    let failed = JsFuture::from(failed).await.unwrap();
    assert_eq!(g(&failed, "status").as_string().as_deref(), Some("failed"));
    assert_eq!(path(&failed, &["error", "code"]).as_string().as_deref(), Some("NotSupported"));
    // DocumentSelectionChanged: the host's events (`__craftOfficeEvent`) reach the handler.
    let handler = recorder("selection");
    let added = Promise::new(&mut |resolve, _| {
        call_js(&doc, "addHandlerAsync", &[s("documentSelectionChanged"), handler.clone(), resolve.into()]);
    });
    assert_eq!(g(&JsFuture::from(added).await.unwrap(), "status").as_string().as_deref(), Some("succeeded"));
    let event: Function = global("__craftOfficeEvent").dyn_into().unwrap();
    event.call1(&JsValue::UNDEFINED, &s(r#"{"type": "documentSelectionChanged"}"#)).unwrap();
    event.call1(&JsValue::UNDEFINED, &s(r#"{"type": "somethingElse"}"#)).unwrap();
    assert_eq!(calls("selection").len(), 1);
    assert_eq!(calls("selection")[0]["type"], "documentSelectionChanged");
    let removed = Promise::new(&mut |resolve, _| {
        call_js(&doc, "removeHandlerAsync", &[s("documentSelectionChanged"), to_js(&json!({})), resolve.into()]);
    });
    assert_eq!(g(&JsFuture::from(removed).await.unwrap(), "status").as_string().as_deref(), Some("succeeded"));
    event.call1(&JsValue::UNDEFINED, &s(r#"{"type": "documentSelectionChanged"}"#)).unwrap();
    assert_eq!(calls("selection").len(), 1);
    // The common API's methods DeckCraft lacks fail through their callback, asynchronously.
    let file = Promise::new(&mut |resolve, _| {
        call_js(&doc, "getFileAsync", &[s("compressed"), to_js(&json!({"sliceSize": 65536})), resolve.into()]);
    });
    assert_eq!(path(&JsFuture::from(file).await.unwrap(), &["error", "code"]).as_string().as_deref(), Some("NotSupported"));
    let bindings = path(&doc, &["bindings"]);
    let bound = Promise::new(&mut |resolve, _| {
        call_js(&bindings, "addFromSelectionAsync", &[s("text"), resolve.into()]);
    });
    assert_eq!(path(&JsFuture::from(bound).await.unwrap(), &["error", "code"]).as_string().as_deref(), Some("NotSupported"));
    // Promise-based APIs reject with NotImplemented.
    let shown = JsFuture::from(call_js(&path(&office, &["addin"]), "showAsTaskpane", &[]).dyn_into::<Promise>().unwrap()).await.unwrap_err();
    assert_eq!(code(&shown), "NotImplemented");
    stop_recording();
}

#[wasm_bindgen_test]
async fn function_commands_run_associated_or_global_functions() {
    setup();
    record_events();
    ready().await;
    // `Office.actions.associate(name, fn)`: fn gets the control id and calls `completed()`.
    let f = Closure::<dyn Fn(JsValue)>::new(|event: JsValue| {
        LOG.with(|l| l.borrow_mut().push(json!({"called": "writeValue", "arg": path(&event, &["source", "id"]).as_string()})));
        call_js(&event, "completed", &[]);
        call_js(&event, "completed", &[]);
    })
    .into_js_value();
    call_js(&path(&global("Office"), &["actions"]), "associate", &[s("writeValue"), f]);
    let run: Function = global("__craftOfficeRunFunction").dyn_into().unwrap();
    run.call1(&JsValue::UNDEFINED, &s(r#"{"call": 7, "name": "writeValue", "control": "Acme.Write"}"#)).unwrap();
    tick().await;
    assert_eq!(calls("writeValue"), vec![json!("Acme.Write")]);
    let done = |call: u64| -> Vec<Json> { posted().into_iter().filter(|e| e["event"] == "functionDone" && e["call"] == call).collect() };
    assert_eq!(done(7), vec![json!({"event": "functionDone", "call": 7})], "once, though completed() was called twice");
    // A function file without `associate`: a global function of that name.
    Reflect::set(&js_sys::global(), &"legacyCommand".into(), &recorder("legacy")).unwrap();
    run.call1(&JsValue::UNDEFINED, &s(r#"{"call": 8, "name": "legacyCommand", "control": "Acme.Legacy"}"#)).unwrap();
    tick().await;
    assert_eq!(calls("legacy")[0]["source"]["id"], "Acme.Legacy");
    // No such function: the host hears so at once (no time-out wait).
    run.call1(&JsValue::UNDEFINED, &s(r#"{"call": 9, "name": "missing", "control": "X"}"#)).unwrap();
    tick().await;
    assert!(done(9)[0]["error"].is_string());
    stop_recording();
}

#[wasm_bindgen_test]
async fn images_slide_ranges_and_go_to() {
    setup();
    ready().await;
    DECK.with(|d| {
        let mut d = d.borrow_mut();
        let id = d.slides[0].id;
        d.selected_slides = vec![id];
    });
    let r = document_async("getSelectedDataAsync", s("slideRange")).await;
    assert_eq!(g(&r, "status").as_string().as_deref(), Some("succeeded"));
    assert_eq!(path(&r, &["value", "slides"]).dyn_into::<Array>().unwrap().length(), 1);
    assert_eq!(
        path(&r, &["value", "slides"])
            .dyn_into::<Array>()
            .unwrap()
            .get(0)
            .dyn_into::<js_sys::Object>()
            .map(|o| from_js(&o)["index"].clone())
            .unwrap(),
        json!(1)
    );
    // setSelectedDataAsync(base64, {coercionType: Image, imageLeft…}, callback).
    let doc = path(&global("Office"), &["context", "document"]);
    let set = Promise::new(&mut |resolve, _| {
        call_js(
            &doc,
            "setSelectedDataAsync",
            &[s("iVBORw0KGgo="), to_js(&json!({"coercionType": "image", "imageLeft": 10, "imageWidth": 50, "other": 1})), resolve.into()],
        );
    });
    assert_eq!(g(&JsFuture::from(set).await.unwrap(), "status").as_string().as_deref(), Some("succeeded"));
    let sent = log().into_iter().rev().find(|a| a["method"] == "setSelectedData").unwrap();
    assert_eq!(sent["args"], json!(["iVBORw0KGgo=", {"coercionType": "image", "imageLeft": 10, "imageWidth": 50}]));
    let go = Promise::new(&mut |resolve, _| {
        call_js(&doc, "goToByIdAsync", &[s("next"), s("index"), resolve.into()]);
    });
    assert_eq!(g(&JsFuture::from(go).await.unwrap(), "status").as_string().as_deref(), Some("succeeded"));
    let sent = log().into_iter().rev().find(|a| a["method"] == "goToById").unwrap();
    assert_eq!(sent["args"], json!(["next", "index"]));
    // Coercions DeckCraft doesn't take fail through the callback.
    let html = Promise::new(&mut |resolve, _| {
        call_js(&doc, "setSelectedDataAsync", &[s("<b>x</b>"), to_js(&json!({"coercionType": "html"})), resolve.into()]);
    });
    assert_eq!(g(&JsFuture::from(html).await.unwrap(), "status").as_string().as_deref(), Some("failed"));
}
