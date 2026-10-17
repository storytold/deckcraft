//! `office.sync`: what an Office add-in's task pane does to the presentation through the
//! PowerPoint JavaScript API's objects.

use serde_json::{Value as Json, json};

use crate::Session;

/// An add-in manifest written for these tests from the public schema.
fn manifest(id: &str, permission: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OfficeApp xmlns="http://schemas.microsoft.com/office/appforoffice/1.1" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="TaskPaneApp">
  <Id>{id}</Id><Version>1.0.0.0</Version><ProviderName>Tests</ProviderName><DefaultLocale>en-US</DefaultLocale>
  <DisplayName DefaultValue="Test pane"/><Description DefaultValue="Tests"/>
  <Hosts><Host Name="Presentation"/></Hosts>
  <DefaultSettings><SourceLocation DefaultValue="https://tests.example/pane.html"/></DefaultSettings>
  <Permissions>{permission}</Permissions>
</OfficeApp>"#
    )
}

/// One Office add-in loaded per test (ids differ, tests run in parallel), unloaded on drop.
struct Addin(String);

impl Addin {
    fn new(id: &str, permission: &str) -> Addin {
        let m = deckcraft_plugin::office::parse_manifest(&manifest(id, permission), deckcraft_plugin::HOST_ID).unwrap();
        let plugin = m.id.clone();
        deckcraft_plugin::install(m, std::env::temp_dir()).unwrap();
        Addin(plugin)
    }
}

impl Drop for Addin {
    fn drop(&mut self) {
        deckcraft_plugin::unload(&self.0);
    }
}

fn sync(s: &mut Session, addin: &Addin, actions: Json) -> Json {
    s.execute("office.sync", &json!({"addin": addin.0, "actions": actions})).unwrap()
}

fn results(s: &mut Session, addin: &Addin, actions: Json) -> Vec<Json> {
    let r = sync(s, addin, actions);
    assert!(r.get("error").is_none(), "{r}");
    r["results"].as_array().cloned().unwrap()
}

fn slides() -> Json {
    json!([{"p": "presentation"}, {"p": "slides"}])
}

/// `presentation.slides` followed by `extra` steps.
fn path(extra: Json) -> Json {
    let mut p = slides().as_array().cloned().unwrap();
    p.extend(extra.as_array().cloned().unwrap());
    Json::Array(p)
}

/// `base` followed by `extra` steps.
fn then(base: &Json, extra: Json) -> Json {
    let mut p = base.as_array().cloned().unwrap();
    p.extend(extra.as_array().cloned().unwrap());
    Json::Array(p)
}

#[test]
fn slides_and_shapes_are_added_read_changed_and_deleted() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000001", "ReadWriteDocument");
    let mut s = Session::with_new();
    let undo = s.active().unwrap().history.undo.len();
    let r = results(
        &mut s,
        &a,
        json!([{"op": "load", "path": slides(), "props": ["count"]}, {"op": "call", "path": slides(), "method": "add", "args": [{}]}]),
    );
    let before = r[0]["count"].as_u64().unwrap();
    let slide = path(json!([{"m": "getItem", "a": [r[1]["id"].as_str().unwrap()]}]));
    let shapes = then(&slide, json!([{"p": "shapes"}]));
    let r = results(
        &mut s,
        &a,
        json!([
            {"op": "load", "path": slides(), "props": ["count"]},
            {"op": "call", "path": shapes, "method": "addTextBox", "args": ["Hello", {"left": 50, "top": 60, "width": 300, "height": 80}]},
            {"op": "call", "path": shapes, "method": "addGeometricShape", "args": ["ellipse", {"left": 400, "top": 100, "width": 120, "height": 120}]},
        ]),
    );
    assert_eq!(r[0]["count"].as_u64().unwrap(), before + 1);
    let text_box = r[1]["id"].as_str().unwrap().to_string();
    let ellipse = r[2]["id"].as_str().unwrap().to_string();
    let shape = |id: &str, rest: Json| then(&then(&shapes, json!([{"m": "getItem", "a": [id]}])), rest);
    let text = shape(&text_box, json!([{"p": "textFrame"}, {"p": "textRange"}]));
    let font = then(&text, json!([{"p": "font"}]));
    let r = results(
        &mut s,
        &a,
        json!([
            {"op": "set", "path": text, "prop": "text", "value": "Hello, world"},
            {"op": "set", "path": font, "prop": "bold", "value": true},
            {"op": "set", "path": font, "prop": "color", "value": "#FF0000"},
            {"op": "set", "path": font, "prop": "size", "value": 32},
            {"op": "set", "path": shape(&text_box, json!([])), "prop": "left", "value": 72},
            {"op": "call", "path": shape(&ellipse, json!([{"p": "fill"}])), "method": "setSolidColor", "args": ["#00FF00"]},
            {"op": "set", "path": shape(&ellipse, json!([])), "prop": "name", "value": "Green dot"},
            {"op": "load", "path": text, "props": ["text", "length"]},
            {"op": "load", "path": font, "props": ["bold", "color", "size", "italic"]},
            {"op": "load", "path": shape(&text_box, json!([])), "props": ["left", "top", "type"]},
            {"op": "load", "path": shape(&ellipse, json!([{"p": "fill"}])), "props": ["type", "foregroundColor"]},
            {"op": "load", "path": shapes, "props": ["items", "name"]},
            {"op": "load", "path": then(&text, json!([{"m": "getSubstring", "a": [7, 5]}])), "props": ["text"]},
        ]),
    );
    assert_eq!(r[7], json!({"text": "Hello, world", "length": 12}));
    assert_eq!(r[8], json!({"bold": true, "color": "#FF0000", "size": 32.0, "italic": false}));
    assert_eq!((r[9]["left"].as_f64(), r[9]["top"].as_f64(), r[9]["type"].as_str()), (Some(72.0), Some(60.0), Some("textBox")));
    assert_eq!(r[10], json!({"type": "solid", "foregroundColor": "#00FF00"}));
    let items = r[11]["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["id"] == json!(ellipse) && i["name"] == "Green dot"), "{items:?}");
    assert_eq!(r[12]["text"], "world");
    // A shape added in the same sync is named by that action's result.
    let r = results(
        &mut s,
        &a,
        json!([
            {"op": "call", "path": shapes, "method": "addGeometricShape", "args": ["star5", {}]},
            {"op": "set", "path": then(&shapes, json!([{"m": "getItem", "a": [{"result": 0}]}])), "prop": "name", "value": "Star"},
            {"op": "load", "path": then(&shapes, json!([{"m": "getItem", "a": [{"result": 0}]}])), "props": ["name", "id"]},
        ]),
    );
    assert_eq!(r[2]["name"], "Star");
    assert_eq!(r[2]["id"], r[0]["id"]);
    s.execute("edit.undo", &json!({})).unwrap();
    // Each sync that changed something is one undo step.
    assert_eq!(s.active().unwrap().history.undo.len(), undo + 3);
    s.execute("edit.undo", &json!({})).unwrap();
    let r = results(&mut s, &a, json!([{"op": "load", "path": text, "props": ["text"]}]));
    assert_eq!(r[0]["text"], "Hello");
    // Deleting a shape, then the slide.
    results(
        &mut s,
        &a,
        json!([{"op": "call", "path": shape(&ellipse, json!([])), "method": "delete"}, {"op": "call", "path": slide, "method": "delete"}]),
    );
    let r = results(&mut s, &a, json!([{"op": "load", "path": slides(), "props": ["count"]}]));
    assert_eq!(r[0]["count"].as_u64().unwrap(), before);
}

#[test]
fn a_failing_action_changes_nothing_and_says_which() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000002", "ReadWriteDocument");
    let mut s = Session::with_new();
    let doc = s.active().unwrap().doc.clone();
    let r = sync(
        &mut s,
        &a,
        json!([
            {"op": "call", "path": slides(), "method": "add", "args": [{}]},
            {"op": "load", "path": path(json!([{"m": "getItem", "a": ["999999"]}])), "props": ["id"]},
        ]),
    );
    assert_eq!(r["error"]["code"], "ItemNotFound", "{r}");
    assert_eq!(r["error"]["action"], 1);
    assert!(std::sync::Arc::ptr_eq(&doc, &s.active().unwrap().doc), "the added slide was taken back");
    for bad in [
        json!({"op": "load", "path": path(json!([{"m": "getItemAt", "a": [-1]}])), "props": ["id"]}),
        json!({"op": "load", "path": path(json!([{"m": "getItemAt", "a": [0]}])), "props": ["nope"]}),
        json!({"op": "load", "path": [{"p": "nowhere"}], "props": ["id"]}),
        json!({"op": "call", "path": path(json!([{"m": "getItemAt", "a": [0]}, {"p": "shapes"}])), "method": "addGeometricShape", "args": ["notAShape"]}),
        json!({"op": "set", "path": path(json!([{"m": "getItemAt", "a": [0]}])), "prop": "id", "value": 3}),
    ] {
        let r = sync(&mut s, &a, json!([bad]));
        assert!(r["error"]["code"].is_string(), "{bad} → {r}");
    }
    // Office.js's GeometricShapeType names map to the presets (msoShapeRightTriangle from VBA).
    assert_eq!(super::preset_of("rightTriangle").ok().as_deref(), Some("rtTriangle"));
}

#[test]
fn permissions_bound_what_an_add_in_may_do() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000003", "ReadDocument");
    let mut s = Session::with_new();
    let r = results(&mut s, &a, json!([{"op": "load", "path": slides(), "props": ["count"]}]));
    assert!(r[0]["count"].as_u64().is_some());
    let r = sync(&mut s, &a, json!([{"op": "call", "path": slides(), "method": "add", "args": [{}]}]));
    assert_eq!(r["error"]["code"], "AccessDenied");
    let none = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000004", "Restricted");
    let r = sync(&mut s, &none, json!([{"op": "load", "path": slides(), "props": ["count"]}]));
    assert_eq!(r["error"]["code"], "AccessDenied");
    // Settings need no permission.
    let settings = json!([{"p": "settings"}]);
    results(&mut s, &none, json!([{"op": "call", "path": settings, "method": "save", "args": [{"k": 1}]}]));
    let r = results(&mut s, &none, json!([{"op": "load", "path": settings, "props": []}]));
    assert_eq!(r[0], json!({"k": 1}));
    assert!(s.execute("office.sync", &json!({"addin": "nope", "actions": []})).is_err());
}

#[test]
fn the_selection_and_the_selected_text() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000005", "ReadWriteDocument");
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.textBox", &json!({"rect": [10, 10, 200, 50], "text": "Pick me"})).unwrap()["id"].as_u64().unwrap();
    s.execute("selection.select", &json!({"ids": [id]})).unwrap();
    let r = results(
        &mut s,
        &a,
        json!([
            {"op": "load", "path": [{"p": "presentation"}, {"m": "getSelectedShapes"}], "props": ["items", "name"]},
            {"op": "load", "path": [{"p": "presentation"}, {"m": "getSelectedSlides"}], "props": ["count"]},
            {"op": "call", "path": [{"p": "document"}], "method": "getSelectedData", "args": ["text"]},
        ]),
    );
    assert_eq!(r[0]["items"][0]["id"], json!(id.to_string()));
    assert_eq!(r[1]["count"], 1);
    assert_eq!(r[2], json!("Pick me"));
    // Layouts by name, to add a slide with one (as VBA's Slides.Add(…, ppLayoutBlank) does).
    let layouts = json!([{"p": "presentation"}, {"p": "slideMasters"}, {"m": "getItemAt", "a": [0]}, {"p": "layouts"}]);
    let r = results(&mut s, &a, json!([{"op": "load", "path": layouts, "props": ["items", "name"]}]));
    let blank = r[0]["items"].as_array().unwrap().iter().find(|l| l["name"] == "Blank").map(|l| l["id"].clone()).unwrap();
    let r = results(&mut s, &a, json!([{"op": "call", "path": slides(), "method": "add", "args": [{"layoutId": blank, "index": 0}]}]));
    let new = r[0]["id"].as_str().unwrap().parse::<u32>().unwrap();
    assert_eq!(s.active().unwrap().doc.slides[0].id.0, new, "added where asked");
    // Text frames are for shapes that hold text.
    let line = results(
        &mut s,
        &a,
        json!([{"op": "call", "path": path(json!([{"m": "getItemAt", "a": [0]}, {"p": "shapes"}])), "method": "addLine", "args": ["straight", {}]}]),
    )[0]["id"]
        .clone();
    let r = sync(
        &mut s,
        &a,
        json!([{"op": "load", "path": path(json!([{"m": "getItemAt", "a": [0]}, {"p": "shapes"}, {"m": "getItem", "a": [line]}, {"p": "textFrame"}])), "props": ["hasText"]}]),
    );
    assert_eq!(r["error"]["code"], "InvalidArgument", "{r}");
    let first = results(&mut s, &a, json!([{"op": "load", "path": slides(), "props": ["items"]}]))[0]["items"][0]["id"].clone();
    results(&mut s, &a, json!([{"op": "call", "path": [{"p": "presentation"}], "method": "setSelectedSlides", "args": [[first]]}]));
    assert_eq!(s.active().unwrap().selection.slide, 0);
}

/// The first shape on the first slide (a text box added for the test) and its id.
fn a_text_box(s: &mut Session, a: &Addin) -> (Json, Json) {
    let shapes = path(json!([{"m": "getItemAt", "a": [0]}, {"p": "shapes"}]));
    let id = results(
        s,
        a,
        json!([{"op": "call", "path": shapes, "method": "addTextBox", "args": ["Hi", {"left": 10, "top": 10, "width": 200, "height": 50}]}]),
    )[0]["id"]
        .clone();
    (then(&shapes, json!([{"m": "getItem", "a": [id.clone()]}])), id)
}

#[test]
fn colours_are_hex_or_css_names_and_read_back_as_hex() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000006", "ReadWriteDocument");
    let mut s = Session::with_new();
    let (shape, _) = a_text_box(&mut s, &a);
    let fill = then(&shape, json!([{"p": "fill"}]));
    let line = then(&shape, json!([{"p": "lineFormat"}]));
    let font = then(&shape, json!([{"p": "textFrame"}, {"p": "textRange"}, {"p": "font"}]));
    let r = results(
        &mut s,
        &a,
        json!([
            {"op": "set", "path": fill, "prop": "foregroundColor", "value": "yellow"},
            {"op": "set", "path": font, "prop": "color", "value": "Blue"},
            {"op": "set", "path": line, "prop": "color", "value": " DarkSlateGray "},
            {"op": "load", "path": fill, "props": ["foregroundColor"]},
            {"op": "load", "path": font, "props": ["color"]},
            {"op": "load", "path": line, "props": ["color"]},
        ]),
    );
    assert_eq!((&r[3]["foregroundColor"], &r[4]["color"], &r[5]["color"]), (&json!("#FFFF00"), &json!("#0000FF"), &json!("#2F4F4F")));
    results(
        &mut s,
        &a,
        json!([{"op": "call", "path": fill, "method": "setSolidColor", "args": ["rebeccapurple"]}, {"op": "set", "path": font, "prop": "color", "value": "#00ff7f"}]),
    );
    let r =
        results(&mut s, &a, json!([{"op": "load", "path": fill, "props": ["foregroundColor"]}, {"op": "load", "path": font, "props": ["color"]}]));
    assert_eq!((&r[0]["foregroundColor"], &r[1]["color"]), (&json!("#663399"), &json!("#00FF7F")));
    // A colour that is neither fails the whole sync, saying which action.
    let r = sync(
        &mut s,
        &a,
        json!([{"op": "set", "path": fill, "prop": "foregroundColor", "value": "red"}, {"op": "set", "path": font, "prop": "color", "value": "blurple"}]),
    );
    assert_eq!((r["error"]["code"].as_str(), r["error"]["action"].as_u64()), (Some("InvalidArgument"), Some(1)), "{r}");
    let r = results(&mut s, &a, json!([{"op": "load", "path": fill, "props": ["foregroundColor"]}]));
    assert_eq!(r[0]["foregroundColor"], "#663399", "the batch was taken back");
}

#[test]
fn tags_on_the_presentation_slides_and_shapes() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000007", "ReadWriteDocument");
    let mut s = Session::with_new();
    let (shape, shape_id) = a_text_box(&mut s, &a);
    for owner in [json!([{"p": "presentation"}]), path(json!([{"m": "getItemAt", "a": [0]}])), shape.clone()] {
        let tags = then(&owner, json!([{"p": "tags"}]));
        let r = results(
            &mut s,
            &a,
            json!([
                {"op": "call", "path": tags, "method": "add", "args": ["customer", "Contoso"]},
                {"op": "call", "path": tags, "method": "add", "args": ["Region", "North"]},
                {"op": "call", "path": tags, "method": "add", "args": ["CUSTOMER", "Fabrikam"]},
                {"op": "load", "path": tags, "props": ["items", "key", "value"]},
                {"op": "load", "path": then(&tags, json!([{"m": "getItem", "a": ["region"]}])), "props": ["key", "value"]},
                {"op": "set", "path": then(&tags, json!([{"m": "getItemAt", "a": [1]}])), "prop": "value", "value": "South"},
                {"op": "call", "path": tags, "method": "delete", "args": ["customer"]},
                {"op": "load", "path": tags, "props": ["count", "items"]},
            ]),
        );
        assert_eq!(
            r[3]["items"],
            json!([{"key": "CUSTOMER", "value": "Fabrikam"}, {"key": "REGION", "value": "North"}]),
            "keys are upper case, add replaces"
        );
        assert_eq!(r[4], json!({"key": "REGION", "value": "North"}));
        assert_eq!(r[7]["items"], json!([{"key": "REGION", "value": "South"}]));
        let r = sync(&mut s, &a, json!([{"op": "load", "path": then(&tags, json!([{"m": "getItem", "a": ["missing"]}])), "props": ["value"]}]));
        assert_eq!(r["error"]["code"], "ItemNotFound");
        let r = sync(&mut s, &a, json!([{"op": "call", "path": tags, "method": "add", "args": ["", "x"]}]));
        assert_eq!(r["error"]["code"], "InvalidArgument");
    }
    // Kept in the model, which saves them (`p:custDataLst`).
    let d = s.active().unwrap();
    assert_eq!(d.doc.tags, vec![("REGION".to_string(), "South".to_string())]);
    let sid = shape_id.as_str().unwrap().parse::<u32>().unwrap();
    assert_eq!(d.doc.slides[0].shape(deckcraft_model::ShapeId(sid)).unwrap().tags.len(), 1);
}

#[test]
fn slides_inserted_from_base64_by_id_after_a_target() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000008", "ReadWriteDocument");
    // A three-slide source, exported as PowerPoint would hand it over.
    let mut src = Session::with_new();
    for t in ["Two", "Three"] {
        src.execute("slide.new", &json!({"title": t})).unwrap();
    }
    let bytes = deckcraft_pptx::export(&src.active().unwrap().doc).unwrap();
    let source = crate::cmd::file::open_presentation("src.pptx", &bytes).unwrap();
    let wanted: Vec<String> = source.slides.iter().skip(1).map(|sl| format!("{}#", sl.id.0)).collect();
    let b64 = crate::cmd::base64_encode(&bytes);
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let first = s.active().unwrap().doc.slides[0].id.0;
    let undo = s.active().unwrap().history.undo.len();
    results(
        &mut s,
        &a,
        json!([{"op": "call", "path": [{"p": "presentation"}], "method": "insertSlidesFromBase64",
            "args": [b64, {"formatting": "KeepSourceFormatting", "sourceSlideIds": wanted, "targetSlideId": format!("{first}#")}]}]),
    );
    let d = s.active().unwrap();
    assert_eq!(d.doc.slides.len(), 4, "two inserted");
    assert_eq!(d.doc.slides[0].id.0, first);
    let titles: Vec<String> = d.doc.slides.iter().map(|sl| sl.title()).collect();
    assert_eq!(titles.get(1..3), Some(&["Two".to_string(), "Three".to_string()][..]), "{titles:?}");
    assert_eq!(d.history.undo.len(), undo + 1, "one undo step");
    let r = sync(
        &mut s,
        &a,
        json!([{"op": "call", "path": [{"p": "presentation"}], "method": "insertSlidesFromBase64", "args": [b64, {"sourceSlideIds": ["999999#"]}]}]),
    );
    assert_eq!(r["error"]["code"], "ItemNotFound", "{r}");
    let r = sync(&mut s, &a, json!([{"op": "call", "path": [{"p": "presentation"}], "method": "insertSlidesFromBase64", "args": ["not base64!"]}]));
    assert!(r["error"]["code"].is_string(), "{r}");
}

#[test]
fn image_and_slide_range_coercions_and_go_to() {
    let a = Addin::new("0d5c7a1e-0000-4bcd-9eaf-000000000009", "ReadWriteDocument");
    let mut s = Session::with_new();
    for _ in 0..2 {
        s.execute("slide.new", &json!({})).unwrap();
    }
    let doc = json!([{"p": "document"}]);
    let go = |s: &mut Session, id: Json, kind: &str| sync(s, &a, json!([{"op": "call", "path": doc, "method": "goToById", "args": [id, kind]}]));
    assert!(go(&mut s, json!("first"), "index").get("error").is_none());
    assert_eq!(s.active().unwrap().selection.slide, 0);
    go(&mut s, json!("next"), "index");
    go(&mut s, json!("next"), "index");
    go(&mut s, json!("next"), "index");
    assert_eq!(s.active().unwrap().selection.slide, 2, "next stops at the last");
    go(&mut s, json!("previous"), "index");
    assert_eq!(s.active().unwrap().selection.slide, 1);
    go(&mut s, json!(3), "index");
    assert_eq!(s.active().unwrap().selection.slide, 2, "slide numbers count from 1");
    let id = s.active().unwrap().doc.slides[0].id.0;
    go(&mut s, json!(id), "slide");
    assert_eq!(s.active().unwrap().selection.slide, 0);
    assert_eq!(go(&mut s, json!(9), "index")["error"]["code"], "ItemNotFound");
    assert_eq!(go(&mut s, json!("x"), "namedItem")["error"]["code"], "NotSupported");
    // The selected slides as a SlideRange: ids as numbers, indexes from 1.
    let r = results(&mut s, &a, json!([{"op": "call", "path": doc, "method": "getSelectedData", "args": ["slideRange"]}]));
    assert_eq!(r[0], json!({"slides": [{"id": id, "title": "", "index": 1}]}));
    let r = sync(&mut s, &a, json!([{"op": "call", "path": doc, "method": "getSelectedData", "args": ["html"]}]));
    assert_eq!(r["error"]["code"], "NotSupported");
    // An image: a picture on the current slide, where asked, keeping its aspect.
    let png = crate::cmd::base64_encode(&[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 2, 0, 0, 0, 75, 109, 41, 220, 0, 0, 0, 17, 73, 68,
        65, 84, 120, 156, 99, 184, 163, 161, 129, 21, 49, 12, 45, 9, 0, 123, 123, 75, 1, 251, 162, 81, 240, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
        130,
    ]);
    let before = s.active().unwrap().doc.slides[0].shapes.len();
    results(
        &mut s,
        &a,
        json!([{"op": "call", "path": doc, "method": "setSelectedData", "args": [png, {"coercionType": "image", "imageLeft": 20, "imageTop": 30, "imageWidth": 100}]}]),
    );
    let d = s.active().unwrap();
    let pic = d.doc.slides[0].shapes.get(before).expect("a picture was added");
    assert!(matches!(pic.kind, deckcraft_model::ShapeKind::Picture { .. }));
    let x = pic.xfrm.unwrap();
    assert_eq!((x.x, x.y, x.w, x.h), (20.0, 30.0, 100.0, 100.0), "an 8×8 picture stays square");
    let r = sync(&mut s, &a, json!([{"op": "call", "path": doc, "method": "setSelectedData", "args": ["AAAA", {"coercionType": "image"}]}]));
    assert!(r["error"]["code"].is_string(), "not a picture: {r}");
}
