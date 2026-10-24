use serde_json::json;

use crate::*;

fn session() -> Session {
    Session::with_new()
}

#[test]
fn new_presentation_and_slides() {
    let mut s = session();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 1);
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 3);
    assert_eq!(s.doc().unwrap().selection.slide, 2);
    s.execute("slide.duplicate", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 4);
    s.execute("slide.move", &json!({"from": 3, "to": 0})).unwrap();
    s.execute("slide.delete", &json!({"index": 0})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 3);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 4);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 3);
}

#[test]
fn text_transform_sets_and_clears_a_warp() {
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 420.0, 160.0], "text": "ARCH"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    let body = |s: &mut Session| s.execute("shape.inspect", &json!({"id": id})).unwrap()["shape"]["text"]["body"].clone();
    assert!(s.execute("format.textWarp", &json!({})).is_err());
    assert!(s.execute("format.textWarp", &json!({"preset": "notAWarp"})).is_err());
    assert!(s.execute("format.textWarp", &json!({"preset": "textArchUp", "adj": "wide"})).is_err());
    assert!(s.execute("format.textWarp", &json!({"preset": "textArchUp", "adj": [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]})).is_err());
    assert!(s.execute("format.textWarp", &json!({"preset": "textArchUp", "adj": [null]})).is_err());
    assert!(body(&mut s)["warp"].is_null());
    s.execute("format.textWarp", &json!({"preset": "textArchUp"})).unwrap();
    assert_eq!(body(&mut s)["warp"], "textArchUp");
    assert!(body(&mut s).get("warpAdj").is_none());
    s.execute("format.textWarp", &json!({"preset": "textInflate", "adj": [20_000.0]})).unwrap();
    let b = body(&mut s);
    assert_eq!(b["warp"], "textInflate");
    assert_eq!(b["warpAdj"], json!([20_000.0]));
    s.execute("format.textWarp", &json!({"preset": "none"})).unwrap();
    assert!(body(&mut s)["warp"].is_null());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(body(&mut s)["warp"], "textInflate");
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("format.textWarp", &json!({"preset": "textCircle"})).unwrap();
    assert_eq!(body(&mut s)["warp"], "textCircle");
}

#[test]
fn insert_shape_and_format() {
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "roundRect", "rect": [100, 100, 200, 100], "text": "Hi"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
    s.execute("shape.line", &json!({"width": 3, "color": "accent2", "dash": "dash"})).unwrap();
    s.execute("shape.rotate", &json!({"deg": 30})).unwrap();
    s.execute("shape.move", &json!({"dx": 10, "dy": -5})).unwrap();
    s.execute("format.bold", &json!({})).unwrap();
    let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
    assert_eq!(v["box"]["x"], 110.0);
    assert_eq!(v["box"]["rot"], 30.0);
    let runs = &v["shape"]["text"]["paragraphs"][0]["runs"][0]["props"];
    assert_eq!(runs["bold"], true);
    // Undo restores step by step.
    for _ in 0..5 {
        s.execute("edit.undo", &json!({})).unwrap();
    }
    let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
    assert!(v["shape"]["fill"].is_null());
}

#[test]
fn text_editing_flow() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Hello world"})).unwrap();
    s.execute("text.move", &json!({"to": "wordLeft", "extend": true})).unwrap();
    s.execute("format.italic", &json!({})).unwrap();
    s.execute("text.move", &json!({"to": "end"})).unwrap();
    s.execute("text.delete", &json!({"dir": "backward"})).unwrap();
    s.execute("text.insert", &json!({"text": "D\nSecond"})).unwrap();
    let t = s.execute("text.get", &json!({"id": title})).unwrap();
    assert_eq!(t["text"], "Hello worlD\nSecond");
    s.execute("text.exit", &json!({})).unwrap();
    assert!(s.doc().unwrap().selection.text.is_none());
    assert_eq!(s.doc().unwrap().selection.shapes.len(), 1);
}

/// `text.set` treats CRLF (or a lone CR) as one paragraph break and keeps no `\r` in the text.
#[test]
fn text_set_crlf_makes_clean_paragraphs() {
    let mut s = session();
    let r = s.execute("insert.textBox", &json!({"text": "x"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.execute("text.set", &json!({"id": id, "text": "one\r\ntwo"})).unwrap();
    let t = s.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(t["paragraphs"][0]["text"], "one");
    assert_eq!(t["paragraphs"][1]["text"], "two");
    assert_eq!(t["text"], "one\ntwo");
    // A lone CR (old Mac line ends) is a paragraph break too, as `edit::insert` treats it.
    s.execute("text.set", &json!({"id": id, "text": "a\rb"})).unwrap();
    let t = s.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(t["text"], "a\nb");
}

/// Cut with only a caret copies nothing, so it must not delete the character after the caret.
#[test]
fn cut_with_only_a_caret_changes_nothing() {
    let mut s = session();
    let r = s.execute("insert.textBox", &json!({"text": "Hello"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("text.move", &json!({"to": "start"})).unwrap();
    let out = s.execute("edit.cut", &json!({})).unwrap();
    assert_eq!(out["text"], "");
    let t = s.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(t["text"], "Hello", "cut with only a caret changed the text");
    // A real selection is still cut.
    s.execute("text.move", &json!({"to": "end", "extend": true})).unwrap();
    let out = s.execute("edit.cut", &json!({})).unwrap();
    assert_eq!(out["text"], "Hello");
    let t = s.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(t["text"], "");
}

#[test]
fn editing_text_refits_a_saved_shrink() {
    use deckcraft_model::text::AutoFit;
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id;
    s.execute("text.edit", &json!({"id": title.0})).unwrap();
    s.execute("text.insert", &json!({"text": "Hi"})).unwrap();
    // As if opened from a file PowerPoint saved shrunk.
    s.edit(|doc, sel| {
        let shapes = crate::shapes_mut(doc, sel).unwrap();
        let sh = deckcraft_model::find_shape_mut(shapes, title).unwrap();
        sh.text.as_mut().unwrap().body.autofit = Some(AutoFit::Shrink { font_scale: Some(0.5), line_reduction: 0.2 });
        Ok(())
    })
    .unwrap();
    s.execute("text.insert", &json!({"text": "!"})).unwrap();
    let sh = s.doc().unwrap().shape(title).cloned().unwrap();
    assert_eq!(sh.text.unwrap().body.autofit, Some(AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 }));
}

/// After an edit, the shrink-on-overflow scale the layout computes is stored, so the saved file
/// carries the scale DeckCraft draws instead of a bare normAutofit (#724).
#[test]
fn editing_shrink_text_stores_the_layout_scale() {
    use deckcraft_model::text::AutoFit;
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 200.0, 40.0]})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    s.execute("format.autofit", &json!({"mode": "shrink"})).unwrap();
    let text = (0..12).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n");
    s.execute("text.set", &json!({"id": id.0, "text": text})).unwrap();
    let sh = s.doc().unwrap().shape(id).cloned().unwrap();
    let Some(AutoFit::Shrink { font_scale, line_reduction }) = sh.text.unwrap().body.autofit else { panic!("shrink") };
    assert!(font_scale.is_some_and(|s| s < 1.0), "the computed fit is stored: {font_scale:?} / {line_reduction}");
    // The saved file must carry the scale DeckCraft draws, so PowerPoint opens it the same way.
    let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut z.by_name("ppt/slides/slide1.xml").unwrap(), &mut xml).unwrap();
    let scales: Vec<i64> = xml.split("fontScale=\"").skip(1).filter_map(|s| s.split('"').next()?.parse().ok()).collect();
    assert!(scales.iter().any(|v| *v < 100_000), "the fitted scale is saved: {scales:?}");
    // Undo restores the state before the edit, and redo brings the stored fit back.
    s.execute("edit.undo", &json!({})).unwrap();
    let sh = s.doc().unwrap().shape(id).cloned().unwrap();
    let Some(AutoFit::Shrink { font_scale, .. }) = sh.text.unwrap().body.autofit else { panic!("shrink") };
    assert_eq!(font_scale, Some(1.0), "undo restores the scale the edit started from");
    s.execute("edit.redo", &json!({})).unwrap();
    let sh = s.doc().unwrap().shape(id).cloned().unwrap();
    let Some(AutoFit::Shrink { font_scale, .. }) = sh.text.unwrap().body.autofit else { panic!("shrink") };
    assert!(font_scale.is_some_and(|v| v < 1.0), "redo brings the stored fit back: {font_scale:?}");
}

#[test]
fn smart_quotes_and_autocorrect() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    for c in ["\"", "h", "i", "\"", " ", "t", "e", "h", " "] {
        s.execute("text.insert", &json!({"text": c})).unwrap();
    }
    let t = s.execute("text.get", &json!({"id": title})).unwrap();
    assert_eq!(t["text"], "“hi” the ");
}

#[test]
fn clipboard_copy_paste_shapes_and_slides() {
    let mut s = session();
    s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [10, 10, 50, 50]})).unwrap();
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let n = s.doc().unwrap().current_slide().unwrap().shapes.len();
    assert_eq!(n, 2 + 3);
    s.execute("edit.copy", &json!({"scope": "slides"})).unwrap();
    s.execute("edit.paste", &json!({"scope": "slides"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 2);
    assert!(s.doc().unwrap().doc.validate().is_empty());
}

#[test]
fn bold_copy_is_html_and_pages_html_pastes_bold() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Hello"})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("format.bold", &json!({"on": true})).unwrap();
    let v = s.execute("edit.copy", &json!({})).unwrap();
    let html = v["html"].as_str().unwrap();
    assert!(html.contains("<b>"), "{html}");
    assert_eq!(v["text"], "Hello");
    // The same plain words are our own clipboard, so a plainer HTML fragment is not reparsed.
    s.execute("text.select", &json!({"anchor": [0, 5], "caret": [0, 5]})).unwrap();
    s.execute("edit.paste", &json!({"text": "Hello", "html": "<p>Hello</p>"})).unwrap();
    let body = s.doc().unwrap().shape(deckcraft_model::ShapeId(title)).unwrap().text.clone().unwrap();
    assert_eq!(body.text(), "HelloHello");
    assert!(body.paragraphs.iter().flat_map(|p| p.runs.iter()).all(|r| r.props.bold == Some(true)), "{body:?}");
    s.execute("text.select", &json!({"anchor": [0, 10], "caret": [0, 10]})).unwrap();
    let caret = s.execute("edit.copy", &json!({})).unwrap();
    assert_eq!(caret["text"], "");
    assert_eq!(caret["html"], "");

    let mut ext = session();
    ext.execute("edit.paste", &json!({"text": "Hello", "html": "<p style=\"font-weight: bold\">Hello</p>"})).unwrap();
    let pasted = ext
        .doc()
        .unwrap()
        .current_slide()
        .unwrap()
        .shapes
        .iter()
        .find(|sh| sh.text.as_ref().is_some_and(|t| t.text() == "Hello"))
        .unwrap()
        .text
        .clone()
        .unwrap();
    assert_eq!(pasted.paragraphs[0].runs[0].props.bold, Some(true), "{pasted:?}");

    let mut mismatched = session();
    mismatched.execute("edit.paste", &json!({"text": "Hi", "html": "<b>Nope</b>"})).unwrap();
    let plain = mismatched
        .doc()
        .unwrap()
        .current_slide()
        .unwrap()
        .shapes
        .iter()
        .find(|sh| sh.text.as_ref().is_some_and(|t| t.text() == "Hi"))
        .unwrap()
        .text
        .clone()
        .unwrap();
    assert_ne!(plain.paragraphs[0].runs[0].props.bold, Some(true));
}

#[test]
fn html_paste_with_a_source_newline_keeps_its_bold() {
    let mut s = session();
    s.execute("edit.paste", &json!({"text": "Hello world", "html": "<p><b>Hello</b>\nworld</p>"})).unwrap();
    let body = s
        .doc()
        .unwrap()
        .current_slide()
        .unwrap()
        .shapes
        .iter()
        .find(|sh| sh.text.as_ref().is_some_and(|t| t.text() == "Hello world"))
        .unwrap()
        .text
        .clone()
        .unwrap();
    assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true), "{body:?}");
}

#[test]
fn html_paste_keeps_a_blank_nbsp_paragraph_and_the_rich_path() {
    for blank in ["<p>&nbsp;</p>", "<p><o:p>&nbsp;</o:p></p>"] {
        let mut s = session();
        let html = format!("<p><b>A</b></p>{blank}<p>B</p>");
        s.execute("edit.paste", &json!({"text": "A\n\u{a0}\nB", "html": html})).unwrap();
        let body = s
            .doc()
            .unwrap()
            .current_slide()
            .unwrap()
            .shapes
            .iter()
            .find(|sh| sh.text.as_ref().is_some_and(|t| t.paragraphs.len() == 3))
            .unwrap()
            .text
            .clone()
            .unwrap();
        assert_eq!(body.paragraphs[0].runs[0].text, "A", "{html}: {body:?}");
        assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true), "{html}: {body:?}");
        assert_eq!(body.paragraphs[2].runs[0].text, "B", "{html}: {body:?}");
    }
}

#[test]
fn group_ungroup_roundtrip_positions() {
    let mut s = session();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 50, 50]})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [300, 200, 50, 50]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
    let g = s.execute("arrange.group", &json!({})).unwrap()["id"].as_u64().unwrap();
    s.execute("shape.move", &json!({"id": g, "dx": 10})).unwrap();
    s.execute("arrange.ungroup", &json!({"ids": [g]})).unwrap();
    let v = s.execute("shape.inspect", &json!({"id": b})).unwrap();
    assert!((v["box"]["x"].as_f64().unwrap() - 310.0).abs() < 1e-6, "{v}");
}

#[test]
fn align_and_distribute() {
    let mut s = session();
    let mut ids = vec![];
    for (x, y) in [(10.0, 10.0), (200.0, 50.0), (500.0, 90.0)] {
        ids.push(s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, y, 40, 40]})).unwrap()["id"].as_u64().unwrap());
    }
    s.execute("edit.select", &json!({"ids": ids})).unwrap();
    s.execute("arrange.align", &json!({"edge": "top"})).unwrap();
    s.execute("arrange.distribute", &json!({"dir": "horizontal"})).unwrap();
    let ys: Vec<f64> = ids.iter().map(|i| s.execute("shape.inspect", &json!({"id": i})).unwrap()["box"]["y"].as_f64().unwrap()).collect();
    assert!(ys.iter().all(|y| (*y - 10.0).abs() < 1e-6));
    let x1 = s.execute("shape.inspect", &json!({"id": ids[1]})).unwrap()["box"]["x"].as_f64().unwrap();
    assert!((x1 - 255.0).abs() < 1e-6, "{x1}");
}

#[test]
fn pointer_draw_move_resize() {
    let mut s = session();
    s.set_tool(ToolKind::Shape { preset: "rect".into() });
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    s.pointer(ev(PointerKind::Down, 100.0, 100.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 200.0, 180.0)).unwrap();
    let r = s.pointer(ev(PointerKind::Up, 200.0, 180.0)).unwrap();
    let id = r["id"].as_u64().unwrap();
    assert_eq!(s.tool.kind, ToolKind::Select);
    // Move by dragging the middle.
    s.prefs.snap_to_grid = false;
    s.prefs.smart_guides = false;
    s.pointer(ev(PointerKind::Down, 150.0, 140.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 170.0, 150.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 170.0, 150.0)).unwrap();
    let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
    assert_eq!(v["box"]["x"], 120.0);
    assert_eq!(v["box"]["y"], 110.0);
    // One undo step per gesture.
    s.execute("edit.undo", &json!({})).unwrap();
    let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
    assert_eq!(v["box"]["x"], 100.0);
    // Resize from the SE handle.
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.pointer(ev(PointerKind::Down, 200.0, 180.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 250.0, 200.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 250.0, 200.0)).unwrap();
    let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
    assert_eq!(v["box"]["w"], 150.0);
}

#[test]
fn click_into_placeholder_starts_editing() {
    let mut s = session();
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    s.pointer(ev(PointerKind::Down, 480.0, 200.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 480.0, 200.0)).unwrap();
    assert!(s.doc().unwrap().selection.text.is_some());
    s.execute("text.insert", &json!({"text": "Typed"})).unwrap();
    assert_eq!(s.doc().unwrap().current_slide().unwrap().title(), "Typed");
}

#[test]
fn sample_deck_builds_and_renders() {
    let mut s = Session::new();
    sample::open_sample(&mut s).unwrap();
    let st = s.doc().unwrap();
    assert!(st.doc.slides.len() >= 8);
    assert!(st.doc.validate().is_empty());
    assert!(!st.is_dirty());
    for i in 0..st.doc.slides.len() {
        let img = deckcraft_render::render_slide(&st.doc, i, &deckcraft_render::RenderOpts { scale: 0.2, ..Default::default() });
        assert_eq!(img.width, 192);
    }
    let v = s.execute("document.inspect", &json!({})).unwrap();
    assert!(v["slides"].as_array().unwrap().len() >= 8);
}

#[test]
fn writable_format_for_path_rejects_formats_deckcraft_does_not_write() {
    use crate::cmd::file::writable_format_for_path as f;
    assert_eq!(f("a.deckcraft"), Some("deckcraft"));
    assert_eq!(f("dir/A.PPTX"), Some("pptx"));
    assert_eq!(f("a.pdf"), Some("pdf"));
    assert_eq!(f("a.jpg"), Some("jpeg"));
    assert_eq!(f("a.txt"), Some("outline"));
    assert_eq!(f("a.gif"), Some("gif"));
    for path in ["a.odp", "a.bmp", "a.svg", "a.outline", "a", "dir.v2/a"] {
        assert_eq!(f(path), None, "{path}");
    }
}

#[test]
fn save_and_reopen_native() {
    let mut s = Session::new();
    sample::open_sample(&mut s).unwrap();
    let bytes = s.execute("file.saveBytes", &json!({})).unwrap()["data"].as_str().unwrap().to_string();
    let n = s.doc().unwrap().doc.slides.len();
    s.execute("file.openBytes", &json!({"name": "x.deckcraft", "data": bytes})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), n);
    assert_eq!(s.documents().len(), 2);
}

#[test]
fn every_command_survives_empty_and_hostile_params() {
    let mut s = Session::new();
    sample::open_sample(&mut s).unwrap();
    s.execute("edit.select", &json!({"ids": [s.doc().unwrap().current_slide().unwrap().shapes[0].id.0]})).unwrap();
    let ids: Vec<&str> = command_specs()
        .iter()
        .map(|c| c.id)
        .filter(|id| !matches!(*id, "file.close" | "file.open" | "file.save" | "file.saveAs" | "file.export" | "file.saveTemplate"))
        .collect();
    for p in [
        json!({}),
        json!(null),
        json!([1, 2]),
        json!({"index": 99999, "ids": [999999], "id": -1, "text": "x", "rect": "bad", "color": 5, "size": 1e308}),
    ] {
        for id in &ids {
            let _ = s.execute(id, &p);
        }
    }
    assert!(s.active().is_some());
    for d in s.documents() {
        assert!(d.doc.validate().is_empty(), "{:?}", d.doc.validate());
    }
}

#[test]
fn base64_roundtrip() {
    for data in [vec![], vec![0u8], vec![1, 2], vec![1, 2, 3], (0..=255u8).collect::<Vec<_>>()] {
        assert_eq!(cmd::base64_decode(&cmd::base64_encode(&data)).unwrap(), data);
    }
    assert!(cmd::base64_decode("!!").is_none());
}

#[test]
fn new_slide_body_tabs_match_text_set() {
    let body = "Install from DMG\nLaunch app\n\tCheck signing\n\t\tDeep\nBuild a deck";
    let mut s = session();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "Test Plan", "body": body})).unwrap();
    let levels = |s: &Session| {
        let slide = s.doc().unwrap().current_slide().unwrap();
        let sh = slide.shapes.iter().find(|x| x.ph_type() == Some(deckcraft_model::PhType::Obj)).unwrap();
        let rows: Vec<(u8, String)> = sh.text.as_ref().unwrap().paragraphs.iter().map(|p| (p.level, p.text())).collect();
        (sh.id.0, rows)
    };
    let (id, from_new) = levels(&s);
    assert_eq!(
        from_new,
        vec![
            (0, "Install from DMG".to_string()),
            (0, "Launch app".to_string()),
            (1, "Check signing".to_string()),
            (2, "Deep".to_string()),
            (0, "Build a deck".to_string()),
        ]
    );
    s.execute("text.set", &json!({"id": id, "text": body})).unwrap();
    assert_eq!(levels(&s).1, from_new);
    s.execute("text.set", &json!({"id": id, "text": "\t\t\t\t\t\t\t\t\tCapped"})).unwrap();
    assert_eq!(levels(&s).1, vec![(8, "Capped".to_string())]);
}

#[test]
fn layout_change_keeps_text() {
    let mut s = session();
    s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "T", "body": "B"})).unwrap();
    s.execute("slide.layout", &json!({"layout": "twoContent"})).unwrap();
    let v = s.execute("slide.inspect", &json!({})).unwrap();
    let texts: Vec<String> = v["shapes"].as_array().unwrap().iter().filter_map(|x| x["text"].as_str().map(String::from)).collect();
    assert!(texts.contains(&"T".to_string()) && texts.contains(&"B".to_string()), "{v}");
    assert_eq!(v["layout"], "Two Content");
}

#[test]
fn themes_and_slide_size() {
    let mut s = session();
    s.execute("design.theme", &json!({"name": "Ember"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.masters[0].theme.name, "Ember");
    s.execute("design.slideSize", &json!({"preset": "standard"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slide_size.width, 720.0);
    s.execute("design.slideSize", &json!({"preset": "widescreen", "orientation": "portrait"})).unwrap();
    assert_eq!((s.doc().unwrap().doc.slide_size.width, s.doc().unwrap().doc.slide_size.height), (540.0, 960.0));
    s.execute("design.slideSize", &json!({"w": 540.0, "h": 960.0, "orientation": "landscape"})).unwrap();
    assert_eq!((s.doc().unwrap().doc.slide_size.width, s.doc().unwrap().doc.slide_size.height), (960.0, 540.0));
    assert!(s.execute("design.slideSize", &json!({"preset": "standard", "orientation": "sideways"})).is_err());
    s.execute("design.headerFooter", &json!({"slideNumber": true, "footer": true, "footerText": "Hello"})).unwrap();
    let v = s.execute("slide.inspect", &json!({})).unwrap();
    assert!(v["shapes"].as_array().unwrap().iter().any(|x| x["placeholder"] == "sldNum"));
}

fn line_ends(s: &Session, id: u64) -> (deckcraft_geom::Point, deckcraft_geom::Point) {
    let st = s.doc().unwrap();
    let sh = st.shape(deckcraft_model::ShapeId(id as u32)).unwrap();
    connect::endpoints(&cmd::xfrm_of(&st.doc, &st.selection, sh))
}

#[test]
fn connectors_follow_glued_shapes() {
    let mut s = session();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 100, 50]})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 300, 100, 50]})).unwrap()["id"].as_u64().unwrap();
    let r = s.execute("shape.connect", &json!({"from": a, "to": b, "preset": "bentConnector3"})).unwrap();
    let c = r["id"].as_u64().unwrap();
    let (p0, p1) = line_ends(&s, c);
    // Closest pair: right side of A → left side of B... or bottom/top; either way on the boxes.
    let sites_a: Vec<Vec<f64>> = serde_json::from_value(s.execute("shape.sites", &json!({"id": a})).unwrap()).unwrap();
    assert!(sites_a.iter().any(|q| (q[0] - p0.x).abs() < 1e-6 && (q[1] - p0.y).abs() < 1e-6));
    // Move B: the connector's end follows, its start stays.
    s.execute("edit.select", &json!({"ids": [b]})).unwrap();
    s.execute("shape.move", &json!({"dx": 50, "dy": 20})).unwrap();
    let (q0, q1) = line_ends(&s, c);
    assert!((q0 - p0).hypot() < 1e-6);
    assert!(((q1 - p1) - deckcraft_geom::Vec2::new(50.0, 20.0)).hypot() < 1e-6, "{p1:?} → {q1:?}");
    // Undo puts both back in one step.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!((line_ends(&s, c).1 - p1).hypot() < 1e-6);
    // Deleting a glued shape unglues that end and leaves the line where it was.
    s.execute("edit.select", &json!({"ids": [a]})).unwrap();
    s.execute("edit.delete", &json!({})).unwrap();
    let st = s.doc().unwrap();
    let sh = st.shape(deckcraft_model::ShapeId(c as u32)).unwrap();
    assert!(matches!(sh.kind, deckcraft_model::ShapeKind::Connector { start: None, end: Some(_) }));
}

#[test]
fn drawing_a_line_between_shapes_glues_it() {
    let mut s = session();
    let a = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [100, 100, 100, 100]})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 100, 100, 100]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.deselect", &json!({})).unwrap();
    s.tool.kind = tools::ToolKind::Shape { preset: "straightConnector1".into() };
    let ev = |kind, x: f64, y: f64| tools::PointerEvent { kind, x, y, mods: Default::default(), tol: 4.0, pressure: None };
    // Right site of the ellipse (200,150) → left site of the rect (400,150), a little off.
    s.pointer(ev(tools::PointerKind::Down, 202.0, 151.0)).unwrap();
    s.pointer(ev(tools::PointerKind::Drag, 300.0, 150.0)).unwrap();
    assert!(!s.tool.sites.is_empty() || s.tool.glue.is_none());
    s.pointer(ev(tools::PointerKind::Drag, 398.0, 149.0)).unwrap();
    assert!(s.tool.glue.is_some());
    s.pointer(ev(tools::PointerKind::Up, 398.0, 149.0)).unwrap();
    let st = s.doc().unwrap();
    let line = st.shapes().iter().find(|x| x.is_line()).unwrap();
    match line.kind {
        deckcraft_model::ShapeKind::Connector { start: Some((x, _)), end: Some((y, _)) } => {
            assert_eq!((x.0 as u64, y.0 as u64), (a, b));
        }
        ref k => panic!("not glued: {k:?}"),
    }
    let id = line.id.0 as u64;
    let (p0, p1) = line_ends(&s, id);
    assert!((p0.x - 200.0).abs() < 1e-6 && (p1.x - 400.0).abs() < 1e-6, "{p0:?} {p1:?}");
}

#[test]
fn freeform_tools_and_command() {
    let mut s = session();
    let before = s.doc().unwrap().shapes().len();
    // Composite command: one undo step.
    let r = s.execute("shape.freeform", &json!({"points": [[100, 100], [200, 120], [150, 220]], "closed": true})).unwrap();
    assert!(r["id"].as_u64().is_some());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().shapes().len(), before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().shapes().len(), before + 1);

    let ev = |kind, x: f64, y: f64| tools::PointerEvent { kind, x, y, mods: Default::default(), tol: 3.0, pressure: None };
    // Polygon: click, click, click, click on the start → closed, filled.
    s.set_tool(tools::ToolKind::Shape { preset: "freeform".into() });
    for (x, y) in [(300.0, 300.0), (400.0, 300.0), (400.0, 400.0)] {
        s.pointer(ev(tools::PointerKind::Down, x, y)).unwrap();
        s.pointer(ev(tools::PointerKind::Up, x, y)).unwrap();
    }
    assert!(s.tool.drawing_freeform());
    s.pointer(ev(tools::PointerKind::Down, 301.0, 301.0)).unwrap();
    assert!(!s.tool.drawing_freeform());
    let st = s.doc().unwrap();
    let sh = st.shapes().last().unwrap();
    match &sh.geom {
        deckcraft_model::Geom::Custom { paths } => assert!(paths[0].d.ends_with('Z') && paths[0].d.matches(" L ").count() == 2, "{}", paths[0].d),
        g => panic!("{g:?}"),
    }
    let x = sh.xfrm.unwrap();
    assert_eq!((x.x, x.y, x.w, x.h), (300.0, 300.0, 100.0, 100.0));

    // Scribble: one drag, open path.
    s.set_tool(tools::ToolKind::Shape { preset: "scribble".into() });
    s.pointer(ev(tools::PointerKind::Down, 10.0, 10.0)).unwrap();
    for i in 1..20 {
        s.pointer(ev(tools::PointerKind::Drag, 10.0 + i as f64 * 5.0, 10.0 + (i as f64).sin() * 10.0)).unwrap();
    }
    let r = s.pointer(ev(tools::PointerKind::Up, 110.0, 10.0)).unwrap();
    assert!(r["id"].as_u64().is_some());
    assert_eq!(s.tool.kind, tools::ToolKind::Select);

    // Curve: clicks then Enter → smooth open path.
    s.set_tool(tools::ToolKind::Shape { preset: "curve".into() });
    for (x, y) in [(500.0, 100.0), (550.0, 50.0), (600.0, 100.0), (650.0, 50.0)] {
        s.pointer(ev(tools::PointerKind::Down, x, y)).unwrap();
        s.pointer(ev(tools::PointerKind::Up, x, y)).unwrap();
    }
    let r = s.key("Enter", Default::default()).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    let st = s.doc().unwrap();
    match &st.shape(id).unwrap().geom {
        deckcraft_model::Geom::Custom { paths } => assert_eq!(paths[0].d.matches(" C ").count(), 3),
        g => panic!("{g:?}"),
    }
}

#[test]
fn double_click_in_table_uses_grown_row_heights() {
    // Rows stored 4 pt tall are drawn tall enough for their text, so 6 pt down is still row 0.
    let mut s = session();
    let data = json!([["a", "b"], ["c", "d"], ["e", "f"]]);
    let id = s.execute("insert.table", &json!({"rows": 3, "cols": 2, "rect": [100.0, 100.0, 400.0, 120.0], "data": data})).unwrap()["id"].clone();
    for row in 0..3 {
        s.execute("table.rowHeight", &json!({"id": id, "row": row, "height": 4.0})).unwrap();
    }
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    s.pointer(ev(PointerKind::DoubleClick, 120.0, 106.0)).unwrap();
    assert_eq!(s.doc().unwrap().selection.text.as_ref().and_then(|t| t.cell), Some((0, 0)));
}

#[test]
fn double_click_edits_text_inside_group() {
    // A text member of a group is still editable: double-click reaches it through the group
    // (regression: it used to be a no-op, so grouped text boxes could not be edited).
    let mut s = session();
    let a =
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [100.0, 100.0, 120.0, 50.0], "text": "Hello"})).unwrap()["id"].as_u64().unwrap();
    let b = s.execute("shape.insert", &json!({"preset": "rect", "rect": [300.0, 200.0, 50.0, 50.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
    s.execute("arrange.group", &json!({})).unwrap();
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    // Double-click the grouped text box: it becomes editable and targets that member.
    s.pointer(ev(PointerKind::DoubleClick, 160.0, 125.0)).unwrap();
    assert_eq!(s.doc().unwrap().selection.text.as_ref().map(|t| t.shape.0 as u64), Some(a));
}

#[test]
fn grown_table_frame_covers_its_drawn_rows() {
    // Rows stored 4 pt tall are drawn taller: the box, its handles and hit tests follow the drawn rows.
    let mut s = session();
    let data = json!([["a", "b"], ["c", "d"], ["e", "f"]]);
    let id = s.execute("insert.table", &json!({"rows": 3, "cols": 2, "rect": [100.0, 100.0, 400.0, 120.0], "data": data})).unwrap()["id"].clone();
    for row in 0..3 {
        s.execute("table.rowHeight", &json!({"id": id, "row": row, "height": 4.0})).unwrap();
    }
    let st = s.doc().unwrap();
    let sh = st.shapes().iter().find(|sh| json!(sh.id) == id).unwrap().clone();
    let deckcraft_model::ShapeKind::Table(t) = &sh.kind else { panic!("not a table") };
    let drawn: f64 = tools::table_rows(st, t).iter().sum();
    assert!(drawn > 12.0, "rows should grow: {drawn}");
    let x = cmd::xfrm_of(&st.doc, &st.selection, &sh);
    assert!((x.h - drawn).abs() < 1e-6, "frame {} vs drawn {drawn}", x.h);
    let below_stored = deckcraft_geom::Point::new(120.0, 100.0 + drawn - 2.0);
    assert!(matches!(s.hit_test(below_stored, 1.0), Some(tools::Hit::Shape { .. })));
}

/// A table at (100, 100): columns `cols` wide, rows 40 pt tall, cells holding `data`.
fn table_at(s: &mut Session, cols: &[f64], data: serde_json::Value) -> deckcraft_model::ShapeId {
    let rows = data.as_array().map_or(1, Vec::len);
    let w: f64 = cols.iter().sum();
    let r =
        s.execute("insert.table", &json!({"rows": rows, "cols": cols.len(), "rect": [100.0, 100.0, w, 40.0 * rows as f64], "data": data})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    for (c, cw) in cols.iter().enumerate() {
        s.execute("table.columnWidth", &json!({"id": id, "col": c, "width": cw})).unwrap();
    }
    s.execute("edit.deselect", &json!({})).unwrap();
    id
}

fn table_of(s: &Session, id: deckcraft_model::ShapeId) -> deckcraft_model::Table {
    match &s.doc().unwrap().shape(id).unwrap().kind {
        deckcraft_model::ShapeKind::Table(t) => t.clone(),
        k => panic!("not a table: {k:?}"),
    }
}

fn tev(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
    PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None }
}

fn text_sel(s: &Session) -> Option<TextSel> {
    s.doc().unwrap().selection.text.clone()
}

/// Slide point of the caret before character `ch` of paragraph `para` in the text being edited.
fn caret_point(s: &Session, para: usize, ch: usize) -> deckcraft_geom::Point {
    let st = s.doc().unwrap();
    text_point(st, st.selection.text.as_ref().unwrap(), para, ch)
}

/// Slide point of the caret before character `ch` of paragraph `para` of text `t`.
fn text_point(st: &DocState, t: &TextSel, para: usize, ch: usize) -> deckcraft_geom::Point {
    let (l, m) = cmd::text::text_frame(st, t).unwrap();
    let (x, top, bot) = l.caret(deckcraft_text::Pos { para, ch }).unwrap();
    m * deckcraft_geom::Point::new(x, (top + bot) / 2.0)
}

#[test]
fn cell_text_is_laid_out_where_the_cell_draws_it() {
    // The caret and hit testing of a cell's text use the cell's rect, not the table's.
    let mut s = session();
    let id = table_at(&mut s, &[200.0, 200.0], json!([["Hello world", "b"], ["c", ""]]));
    s.execute("text.edit", &json!({"id": id, "cell": [0, 1], "at": [0, 0]})).unwrap();
    let c = caret_point(&s, 0, 0);
    assert!(c.x > 300.0 && c.x < 320.0 && c.y > 100.0 && c.y < 140.0, "caret of cell (0, 1) at {c:?}");
    // An empty cell still has a caret.
    s.execute("text.edit", &json!({"id": id, "cell": [1, 1]})).unwrap();
    let c = caret_point(&s, 0, 0);
    assert!(c.x > 300.0 && c.y > 140.0 && c.y < 180.0, "caret of empty cell (1, 1) at {c:?}");
    // Hit testing: the start and the end of "Hello world".
    let st = s.doc().unwrap();
    let t = TextSel { shape: id, cell: Some((0, 0)), ..Default::default() };
    assert_eq!(cmd::text::text_pos_at(st, &t, deckcraft_geom::Point::new(101.0, 115.0)), (0, 0));
    assert_eq!(cmd::text::text_pos_at(st, &t, deckcraft_geom::Point::new(295.0, 115.0)), (0, 11));
}

#[test]
fn double_click_in_a_cell_selects_the_word_clicked() {
    let mut s = session();
    let id = table_at(&mut s, &[200.0, 200.0], json!([["Hello world", "b"], ["c", "d"]]));
    // Where "world" is drawn.
    s.execute("text.edit", &json!({"id": id, "cell": [0, 0]})).unwrap();
    let p = caret_point(&s, 0, 8);
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("edit.deselect", &json!({})).unwrap();
    // As the UI sends it: press, release, press, double-click (on release), then the release.
    s.pointer(tev(PointerKind::Down, p.x, p.y)).unwrap();
    s.pointer(tev(PointerKind::Up, p.x, p.y)).unwrap();
    assert!(text_sel(&s).is_none(), "a click on an unselected table selects it");
    s.pointer(tev(PointerKind::Down, p.x, p.y)).unwrap();
    s.pointer(tev(PointerKind::DoubleClick, p.x, p.y)).unwrap();
    s.pointer(tev(PointerKind::Up, p.x, p.y)).unwrap();
    let t = text_sel(&s).unwrap();
    assert_eq!((t.cell, t.ordered()), (Some((0, 0)), ((0, 6), (0, 11))), "the word under the double-click");
    // The control channel sends the double-click alone.
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("edit.deselect", &json!({})).unwrap();
    s.pointer(tev(PointerKind::DoubleClick, p.x, p.y)).unwrap();
    assert_eq!(text_sel(&s).map(|t| t.ordered()), Some(((0, 6), (0, 11))));
}

#[test]
fn clicks_while_editing_a_cell_move_the_caret_between_cells() {
    let mut s = session();
    let id = table_at(&mut s, &[200.0, 200.0], json!([["Hello world", "Second cell"], ["c", "d"]]));
    s.execute("text.edit", &json!({"id": id, "cell": [0, 1], "at": [0, 0]})).unwrap();
    let p6 = caret_point(&s, 0, 6);
    let p11 = caret_point(&s, 0, 11);
    s.execute("text.edit", &json!({"id": id, "cell": [1, 0]})).unwrap();
    // A click in another cell of the table moves the caret there, at the click.
    s.pointer(tev(PointerKind::Down, p6.x, p6.y)).unwrap();
    let t = text_sel(&s).unwrap();
    assert_eq!((t.shape, t.cell, t.caret, t.anchor), (id, Some((0, 1)), (0, 6), (0, 6)));
    // Dragging selects within that cell, even past its edge.
    s.pointer(tev(PointerKind::Drag, p11.x, p11.y)).unwrap();
    s.pointer(tev(PointerKind::Drag, 600.0, p11.y)).unwrap();
    s.pointer(tev(PointerKind::Up, 600.0, p11.y)).unwrap();
    let t = text_sel(&s).unwrap();
    assert_eq!((t.cell, t.ordered()), (Some((0, 1)), ((0, 6), (0, 11))));
    // Shift+click in another cell starts a new caret there instead of extending across cells.
    let shift = Mods { shift: true, ..Default::default() };
    s.pointer(PointerEvent { mods: shift, ..tev(PointerKind::Down, 150.0, 165.0) }).unwrap();
    s.pointer(tev(PointerKind::Up, 150.0, 165.0)).unwrap();
    let t = text_sel(&s).unwrap();
    assert_eq!((t.cell, t.is_range()), (Some((1, 0)), false));
    // Typing goes into the clicked cell.
    s.execute("text.insert", &json!({"text": "X"})).unwrap();
    assert_eq!(table_of(&s, id).cell(1, 0).unwrap().text.text(), "cX");
    // A click outside the table stops editing.
    s.pointer(tev(PointerKind::Down, 50.0, 400.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 50.0, 400.0)).unwrap();
    assert!(text_sel(&s).is_none());
}

#[test]
fn a_click_in_a_selected_table_enters_the_clicked_cell() {
    let mut s = session();
    let id = table_at(&mut s, &[200.0, 200.0], json!([["a", "b"], ["c", "Hello world"]]));
    s.pointer(tev(PointerKind::Down, 350.0, 160.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 350.0, 160.0)).unwrap();
    assert_eq!((s.doc().unwrap().selection.shapes.clone(), text_sel(&s)), (vec![id], None), "the first click selects the table");
    let st = s.doc().unwrap();
    let p = text_point(st, &TextSel { shape: id, cell: Some((1, 1)), ..Default::default() }, 0, 6);
    s.pointer(tev(PointerKind::Down, p.x, p.y)).unwrap();
    s.pointer(tev(PointerKind::Up, p.x, p.y)).unwrap();
    let t = text_sel(&s).unwrap();
    assert_eq!((t.cell, t.caret), (Some((1, 1)), (0, 6)), "caret at the click");
    // Dragging a selected table still moves it.
    s.execute("text.exit", &json!({})).unwrap();
    s.pointer(tev(PointerKind::Down, 150.0, 120.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 170.0, 130.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 170.0, 130.0)).unwrap();
    assert!(text_sel(&s).is_none());
    assert_ne!(s.doc().unwrap().shape(id).unwrap().xfrm.unwrap().x, 100.0);
}

#[test]
fn caret_moves_by_line_in_a_wrapped_cell() {
    let mut s = session();
    let id = table_at(&mut s, &[90.0, 200.0], json!([["one two three four five six seven", "b"]]));
    s.execute("text.edit", &json!({"id": id, "cell": [0, 0], "at": [0, 0]})).unwrap();
    s.execute("text.move", &json!({"to": "down"})).unwrap();
    let down = text_sel(&s).unwrap().caret;
    assert!(down.0 == 0 && down.1 > 0, "down goes to the next line of the paragraph: {down:?}");
    s.execute("text.move", &json!({"to": "lineEnd"})).unwrap();
    let end = text_sel(&s).unwrap().caret;
    assert!(end.1 > down.1 && end.1 < 33, "end of the second line: {end:?}");
    s.execute("text.move", &json!({"to": "up"})).unwrap();
    assert!(text_sel(&s).unwrap().caret.1 < down.1);
}

#[test]
fn table_borders_are_hit_where_they_are_drawn() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0, 100.0], json!([["a", "b", "c"], ["d", "e", "f"]]));
    let hit = |s: &Session, x: f64, y: f64| s.hit_test(deckcraft_geom::Point::new(x, y), 3.0);
    assert_eq!(hit(&s, 201.0, 125.0), Some(tools::Hit::TableColumn { id, col: 0 }));
    assert_eq!(hit(&s, 400.0, 125.0), Some(tools::Hit::TableColumn { id, col: 2 }), "the right edge");
    assert_eq!(hit(&s, 150.0, 139.0), Some(tools::Hit::TableRow { id, row: 0 }));
    assert_eq!(hit(&s, 150.0, 181.0), Some(tools::Hit::TableRow { id, row: 1 }), "the bottom edge");
    assert!(matches!(hit(&s, 100.0, 125.0), Some(tools::Hit::Shape { .. })), "the left edge moves the table");
    assert!(matches!(hit(&s, 150.0, 125.0), Some(tools::Hit::Shape { .. })));
    // Hovering a border shows it (the UI picks the resize cursor from the hover).
    s.pointer(tev(PointerKind::Move, 300.0, 170.0)).unwrap();
    assert_eq!(s.tool.hover, Some(tools::Hit::TableColumn { id, col: 1 }));
    // A border inside a merged cell isn't there.
    s.execute("table.merge", &json!({"id": id, "from": [0, 0], "to": [0, 1]})).unwrap();
    assert!(matches!(hit(&s, 200.0, 120.0), Some(tools::Hit::Shape { .. })));
    assert_eq!(hit(&s, 200.0, 160.0), Some(tools::Hit::TableColumn { id, col: 0 }));
}

#[test]
fn dragging_column_borders_resizes_columns() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0, 100.0], json!([["a", "b", "c"], ["d", "e", "f"]]));
    let undo_depth = s.doc().unwrap().history.undo.len();
    // An inner border: the two columns share the change, the table keeps its width.
    s.pointer(tev(PointerKind::Down, 200.0, 125.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 215.0, 125.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 230.0, 128.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 230.0, 128.0)).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![130.0, 70.0, 100.0]);
    assert_eq!(s.doc().unwrap().shape(id).unwrap().xfrm.unwrap().w, 300.0);
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_depth + 1, "one undo step per drag");
    // Columns don't collapse.
    s.pointer(tev(PointerKind::Down, 230.0, 125.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 500.0, 125.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 500.0, 125.0)).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![200.0 - cmd::table::MIN_COLUMN_WIDTH, cmd::table::MIN_COLUMN_WIDTH, 100.0]);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![100.0, 100.0, 100.0]);
    // The right edge: the last column and the table get wider.
    s.pointer(tev(PointerKind::Down, 400.0, 120.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 450.0, 120.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 450.0, 120.0)).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![100.0, 100.0, 150.0]);
    assert_eq!(s.doc().unwrap().shape(id).unwrap().xfrm.unwrap().w, 350.0);
    // A click on a border without dragging changes nothing.
    let before = s.doc().unwrap().doc.clone();
    s.pointer(tev(PointerKind::Down, 200.0, 125.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 200.0, 125.0)).unwrap();
    assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().doc));
}

#[test]
fn dragging_a_row_border_sets_the_row_height() {
    let mut s = session();
    let id = table_at(&mut s, &[200.0, 200.0], json!([["a", "b"], ["c", "d"]]));
    let drawn = |s: &Session| {
        let st = s.doc().unwrap();
        tools::table_rows(st, &table_of(s, id))
    };
    s.pointer(tev(PointerKind::Down, 150.0, 140.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 150.0, 170.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 150.0, 170.0)).unwrap();
    assert_eq!(drawn(&s), vec![70.0, 40.0]);
    let st = s.doc().unwrap();
    assert_eq!(cmd::xfrm_of(&st.doc, &st.selection, st.shape(id).unwrap()).h, 110.0, "the table grows");
    // Rows still fit their text: dragging the border up stops at the text.
    s.pointer(tev(PointerKind::Down, 150.0, 170.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 150.0, 60.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 150.0, 60.0)).unwrap();
    let h = drawn(&s)[0];
    assert!(h > 10.0 && h < 40.0, "row 0 shrinks to its text: {h}");
    // The bottom edge sizes the last row.
    let bottom = 100.0 + drawn(&s).iter().sum::<f64>();
    s.pointer(tev(PointerKind::Down, 150.0, bottom)).unwrap();
    s.pointer(tev(PointerKind::Drag, 150.0, bottom + 20.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 150.0, bottom + 20.0)).unwrap();
    assert_eq!(drawn(&s)[1], 60.0);
}

#[test]
fn dragging_a_border_while_editing_a_cell_keeps_editing() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0], json!([["a", "b"]]));
    s.execute("text.edit", &json!({"id": id, "cell": [0, 1]})).unwrap();
    s.pointer(tev(PointerKind::Down, 200.0, 120.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 180.0, 120.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 180.0, 120.0)).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![80.0, 120.0]);
    assert_eq!(text_sel(&s).and_then(|t| t.cell), Some((0, 1)));
}

#[test]
fn column_width_can_keep_the_table_width() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0, 100.0], json!([["a", "b", "c"]]));
    s.execute("table.columnWidth", &json!({"id": id, "col": 1, "width": 150.0, "keepTotal": true})).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![100.0, 150.0, 50.0]);
    s.execute("table.columnWidth", &json!({"id": id, "col": 1, "width": 1000.0, "keepTotal": true})).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![100.0, 200.0 - cmd::table::MIN_COLUMN_WIDTH, cmd::table::MIN_COLUMN_WIDTH]);
    // The last column has no neighbour: the table changes width.
    s.execute("table.columnWidth", &json!({"id": id, "col": 2, "width": 80.0, "keepTotal": true})).unwrap();
    assert_eq!(s.doc().unwrap().shape(id).unwrap().xfrm.unwrap().w, 100.0 + (200.0 - cmd::table::MIN_COLUMN_WIDTH) + 80.0);
}

#[test]
fn rotated_tables_hit_the_cell_under_the_pointer() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0], json!([["a", "b"], ["c", "d"]]));
    s.execute("shape.rotate", &json!({"ids": [id], "deg": 90})).unwrap();
    let st = s.doc().unwrap();
    let f = cmd::table::table_frame(st, st.shape(id).unwrap()).unwrap();
    let p = f.affine * deckcraft_geom::Point::new(150.0, 60.0);
    s.pointer(tev(PointerKind::DoubleClick, p.x, p.y)).unwrap();
    assert_eq!(text_sel(&s).and_then(|t| t.cell), Some((1, 1)));
}

#[test]
fn tab_moves_between_cells_skipping_merged_ones() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0, 100.0], json!([["a", "b", "c"], ["d", "e", "f"]]));
    s.execute("table.merge", &json!({"id": id, "from": [0, 0], "to": [0, 1]})).unwrap();
    s.execute("text.edit", &json!({"id": id, "cell": [0, 0]})).unwrap();
    let cell = |s: &Session| text_sel(s).and_then(|t| t.cell);
    let shift = Mods { shift: true, ..Default::default() };
    s.key("Tab", Mods::default()).unwrap();
    assert_eq!(cell(&s), Some((0, 2)), "the cell under the merge is skipped");
    s.key("Tab", shift).unwrap();
    assert_eq!(cell(&s), Some((0, 0)));
    s.key("Tab", shift).unwrap();
    assert_eq!(cell(&s), Some((0, 0)), "Shift+Tab in the first cell stays");
    s.execute("text.edit", &json!({"id": id, "cell": [1, 2]})).unwrap();
    s.key("Tab", Mods::default()).unwrap();
    assert_eq!((cell(&s), table_of(&s, id).n_rows()), (Some((2, 0)), 3), "Tab in the last cell adds a row");
}

#[test]
fn right_to_left_paragraphs() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    // Explicitly left-to-right first, so typing Arabic doesn't already turn it right-to-left (#169).
    s.execute("format.rtl", &json!({"on": false})).unwrap();
    s.execute("text.insert", &json!({"text": "سنة 2024 (Retrait) فقط"})).unwrap();
    s.execute("format.rtl", &json!({})).unwrap();
    let para = |s: &Session| s.doc().unwrap().current_slide().unwrap().shapes[0].text.as_ref().unwrap().paragraphs[0].props.clone();
    assert_eq!(para(&s).rtl, Some(true));
    // Left (the default) becomes right: the text stays at its start edge.
    assert_eq!(para(&s).align, Some(deckcraft_model::text::Align::Right));
    s.execute("format.align", &json!({"align": "justifyLow"})).unwrap();
    assert_eq!(para(&s).align, Some(deckcraft_model::text::Align::JustLow));
    // Toggling back keeps a justified alignment.
    s.execute("format.rtl", &json!({})).unwrap();
    assert_eq!(para(&s).rtl, Some(false));
    assert_eq!(para(&s).align, Some(deckcraft_model::text::Align::JustLow));
    s.execute("format.rtl", &json!({"on": true})).unwrap();
    s.execute("format.alignLeft", &json!({})).unwrap();
    s.execute("format.rtl", &json!({"on": false})).unwrap();
    assert_eq!(para(&s).align, Some(deckcraft_model::text::Align::Right));
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(para(&s).rtl, Some(true));
}

#[test]
fn typing_kurdish_or_arabic_auto_detects_rtl_and_advances_caret_smoothly() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();

    let mut first_x = None;
    let mut last_x = 0.0;

    // Type Kurdish sentence character-by-character: "من ناوم هێڤاره"
    for ch in ["م", "ن", " ", "ن", "ا", "و", "م", " ", "ه", "ێ", "ڤ", "ا", "ر", "ه"] {
        s.execute("text.insert", &json!({"text": ch})).unwrap();
        let st = s.doc().unwrap();
        let t = st.selection.text.as_ref().unwrap();
        let l = super::cmd::text::layout_for(st, t).unwrap();
        let (cx, ..) = l.caret(deckcraft_text::Pos { para: t.caret.0, ch: t.caret.1 }).unwrap();

        if first_x.is_none() {
            first_x = Some(cx);
        }
        last_x = cx;

        // The title is centred: the caret must NOT jump back to the left margin (~7.2 pt).
        assert!(cx > 100.0, "caret should not jump to left margin: {cx}");
    }

    // Caret advances leftwards overall as text is appended
    assert!(last_x < first_x.unwrap(), "caret should advance leftwards: last={last_x} first={:?}", first_x);
    let para = s.doc().unwrap().current_slide().unwrap().shapes[0].text.as_ref().unwrap().paragraphs[0].props.clone();
    assert_eq!(para.rtl, Some(true), "auto-detected as RTL");
    assert_eq!(para.align, None, "the title keeps the alignment it inherits");

    // A text box inherits the deck's default (left) alignment, which it keeps too.
    s.execute("text.exit", &json!({})).ok();
    let id = s.execute("insert.textBox", &json!({"rect": [10, 10, 300, 100], "text": ""})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id})).unwrap();
    s.execute("text.insert", &json!({"text": "مرحبا"})).unwrap();
    let st = s.doc().unwrap();
    let sh = st.shape(deckcraft_model::ShapeId(id as u32)).unwrap();
    let para = sh.text.as_ref().unwrap().paragraphs[0].props.clone();
    assert_eq!(para.rtl, Some(true));
    assert_eq!(para.align, None, "the text box keeps the alignment it inherits");
}

#[test]
fn explicit_ltr_override_is_preserved_when_typing_rtl() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("format.rtl", &json!({"on": false})).unwrap();
    s.execute("text.insert", &json!({"text": "مرحبا"})).unwrap();
    let para = s.doc().unwrap().current_slide().unwrap().shapes[0].text.as_ref().unwrap().paragraphs[0].props.clone();
    assert_eq!(para.rtl, Some(false), "user explicit LTR is preserved");
}

#[test]
fn header_footer_fixed_date_is_the_slides_own_text() {
    use deckcraft_model::text::RunKind;
    let mut s = session();
    let date = |s: &Session| {
        let slide = s.doc().unwrap().current_slide().unwrap();
        slide.shapes.iter().find(|x| x.ph_type() == Some(deckcraft_model::PhType::Date)).and_then(|x| x.text.clone()).unwrap()
    };
    let kind = |s: &Session| date(s).paragraphs[0].runs[0].kind.clone();
    // Automatic: a date field, saved with today's date.
    s.execute("design.headerFooter", &json!({"date": true})).unwrap();
    assert_eq!(kind(&s), RunKind::Field { field: "datetime1".into() });
    assert_eq!(date(&s).text().matches('/').count(), 2, "M/D/YYYY");
    // Fixed: plain text in the slide's own placeholder (other slides' fields aren't affected).
    s.execute("design.headerFooter", &json!({"date": true, "dateText": "Launch day"})).unwrap();
    assert_eq!((kind(&s), date(&s).text()), (RunKind::Text, "Launch day".to_string()));
    // Back to automatic.
    s.execute("design.headerFooter", &json!({"date": true, "dateText": ""})).unwrap();
    assert!(matches!(kind(&s), RunKind::Field { .. }));
    // A new date field is saved with the date it shows.
    s.execute("text.edit", &json!({"id": s.doc().unwrap().current_slide().unwrap().shapes[0].id.0})).unwrap();
    s.execute("text.insert", &json!({"text": "Updated "})).unwrap();
    s.execute("insert.dateTime", &json!({"format": "datetime10"})).unwrap();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].text.clone().unwrap();
    let run = title.paragraphs[0].runs.iter().find(|r| matches!(r.kind, RunKind::Field { .. })).cloned().unwrap();
    assert!(run.text.contains(':'), "H:mm {run:?}");
}

/// (text, linked?) of each run in the first paragraph of the current slide's title.
fn title_runs(s: &Session) -> Vec<(String, bool)> {
    let sh = &s.doc().unwrap().current_slide().unwrap().shapes[0];
    let p = &sh.text.as_ref().unwrap().paragraphs[0];
    p.runs.iter().map(|r| (r.text.clone(), r.props.link.is_some())).collect()
}

#[test]
fn hyperlink_with_a_bare_caret_inserts_the_address() {
    // #31: with the caret at the end of the text, Insert Hyperlink used to format an empty range
    // and change nothing.
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "See "})).unwrap();
    s.execute("insert.hyperlink", &json!({"url": "https://example.org"})).unwrap();
    s.execute("text.insert", &json!({"text": " now"})).unwrap();
    assert_eq!(
        title_runs(&s),
        vec![("See ".into(), false), ("https://example.org".into(), true), (" now".into(), false)],
        "the address is inserted as a link, and typing after it doesn't extend it"
    );
}

#[test]
fn hyperlink_display_text_and_targets() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("insert.hyperlink", &json!({"url": "mailto:a@b.org"})).unwrap();
    assert_eq!(title_runs(&s), vec![("a@b.org".into(), true)], "email links show the address without mailto:");
    s.execute("text.insert", &json!({"text": " "})).unwrap();
    s.execute("insert.hyperlink", &json!({"slide": 0, "text": "start"})).unwrap();
    assert_eq!(title_runs(&s), vec![("a@b.org".into(), true), (" ".into(), false), ("start".into(), true)]);
}

#[test]
fn hyperlink_on_a_word_or_shape_links_it() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Hello world"})).unwrap();
    s.execute("text.move", &json!({"to": "left"})).unwrap();
    s.execute("insert.hyperlink", &json!({"url": "https://example.org"})).unwrap();
    assert_eq!(title_runs(&s), vec![("Hello ".into(), false), ("world".into(), true)], "a caret inside a word links the word");
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("insert.hyperlink", &json!({"url": "https://example.org"})).unwrap();
    assert!(s.doc().unwrap().current_slide().unwrap().shapes[0].click.is_some(), "a selected shape gets the link as its click action");
}

#[test]
fn hyperlink_can_keep_highlight_click() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("insert.hyperlink", &json!({"url": "https://example.org", "tooltip": "tip", "highlightClick": true})).unwrap();
    let click = s.doc().unwrap().current_slide().unwrap().shapes[0].click.clone().unwrap();
    assert_eq!(click.tooltip, "tip");
    assert!(click.highlight_click);
    assert_eq!(click.action, model::text::Action::Url { url: "https://example.org".into() });
    s.execute("insert.hyperlink", &json!({"url": "https://example.org"})).unwrap();
    let click = s.doc().unwrap().current_slide().unwrap().shapes[0].click.clone().unwrap();
    assert!(!click.highlight_click, "omitting the flag still stores false");
}

/// A .pptx (base64) whose first slide's title is set in `font`.
fn deck_in_font(font: &str) -> String {
    let mut p = deckcraft_model::Presentation::default();
    let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
    let mut body = deckcraft_model::text::TextBody::from_text("Hello");
    body.paragraphs[0].runs[0].props.font = Some(font.into());
    slide.shapes[0].text = Some(body);
    crate::cmd::base64_encode(&crate::cmd::file::save_bytes(&p, "pptx").unwrap())
}

#[test]
fn opening_a_file_lists_the_fonts_it_uses_that_are_missing() {
    let mut s = Session::with_new();
    let r = s.execute("file.openBytes", &json!({"name": "a.pptx", "data": deck_in_font("Qqq Nonexistent Sans")})).unwrap();
    assert_eq!(r["missingFonts"], json!(["Qqq Nonexistent Sans"]));
    assert_eq!(s.doc().unwrap().missing_fonts, vec!["Qqq Nonexistent Sans".to_string()]);
    // A font that is there (bundled or installed) isn't listed; new documents list nothing.
    let have = deckcraft_fonts::FontDb::global().face(deckcraft_fonts::FALLBACK_FAMILY, "Regular").family.clone();
    let r = s.execute("file.openBytes", &json!({"name": "b.pptx", "data": deck_in_font(&have)})).unwrap();
    assert_eq!(r["missingFonts"], json!([]));
    s.execute("file.new", &json!({})).unwrap();
    assert!(s.doc().unwrap().missing_fonts.is_empty());
}

#[test]
fn an_embedded_face_is_used_when_the_machine_lacks_it() {
    let mut p = deckcraft_model::Presentation::default();
    let slide = std::sync::Arc::make_mut(&mut p.slides[0]);
    let mut body = deckcraft_model::text::TextBody::from_text("Hello");
    body.paragraphs[0].runs[0].props.font = Some("DeckCraft Colr Squares".into());
    slide.shapes[0].text = Some(body);
    let id = p
        .add_media("font1.fntdata", "application/x-fontdata", include_bytes!("../../render/tests/fixtures/colr-squares.ttf").to_vec())
        .expect("media");
    p.embedded_fonts.push(deckcraft_model::EmbeddedFont { typeface: "DeckCraft Colr Squares".into(), regular: Some(id), ..Default::default() });
    let data = crate::cmd::base64_encode(&crate::cmd::file::save_bytes(&p, "pptx").unwrap());
    let mut s = Session::with_new();
    let r = s.execute("file.openBytes", &json!({"name": "fonts.pptx", "data": data})).unwrap();
    assert_eq!(r["missingFonts"], json!([]));
    let font = s.doc().unwrap().doc.embedded_fonts.first().unwrap();
    assert_eq!(font.typeface, "DeckCraft Colr Squares");
    assert!(font.regular.is_some());
}

#[test]
fn clicks_in_flipped_shapes_hit_the_text_where_it_is_drawn() {
    // Flips don't mirror text, so a click lands on the character drawn under it.
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 400, 100], "text": "Hello flipped world"})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    let pos = |s: &Session, x: f64, y: f64| {
        let st = s.doc().unwrap();
        tools::text_pos(st, st.shape(id).unwrap(), deckcraft_geom::Point::new(x, y))
    };
    let xs = [240.0, 300.0, 360.0];
    let plain = xs.map(|x| pos(&s, x, 150.0));
    assert!(plain[0].1 < plain[1].1 && plain[1].1 < plain[2].1, "{plain:?}");
    s.execute("shape.flip", &json!({"axis": "horizontal"})).unwrap();
    assert_eq!(xs.map(|x| pos(&s, x, 150.0)), plain);
    // flipV turns the text 180° about the box centre (300, 150).
    s.execute("shape.flip", &json!({"axis": "horizontal"})).unwrap();
    s.execute("shape.flip", &json!({"axis": "vertical"})).unwrap();
    assert_eq!(xs.map(|x| pos(&s, 600.0 - x, 150.0)), plain);
}

#[test]
fn inserted_hyperlinks_look_like_links() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Read the docs"})).unwrap();
    s.execute("text.move", &json!({"to": "wordLeft", "extend": true})).unwrap();
    assert_eq!(s.execute("format.state", &json!({})).unwrap()["underline"], false);
    s.execute("insert.hyperlink", &json!({"url": "https://example.org"})).unwrap();
    // Underlined (and hlink-coloured: same resolution as files) without touching the run's own props.
    assert_eq!(s.execute("format.state", &json!({})).unwrap()["underline"], true);
    let body = s.doc().unwrap().current_slide().unwrap().shapes[0].text.clone().unwrap();
    let run = body.paragraphs[0].runs.iter().find(|r| r.props.link.is_some()).unwrap();
    assert!(run.props.fill.is_none() && run.props.underline.is_none(), "{run:?}");
    // Underline off is the run's own u="none", as in PowerPoint.
    s.execute("format.underline", &json!({})).unwrap();
    assert_eq!(s.execute("format.state", &json!({})).unwrap()["underline"], false);
}

/// #270: shapes off the slide (in the editor's pasteboard) are hit, selected, moved and marqueed
/// like any other; one can be dragged back onto the slide or out of it.
#[test]
fn shapes_off_the_slide_can_be_selected_and_moved() {
    let mut s = session();
    s.prefs.snap_to_grid = false;
    s.prefs.smart_guides = false;
    let off = s.execute("shape.insert", &json!({"preset": "rect", "rect": [1000.0, 100.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
    let half = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [-60.0, 300.0, 120.0, 80.0]})).unwrap()["id"].as_u64().unwrap();
    // Inserting off the slide keeps the box where it was asked for.
    assert_eq!(s.execute("shape.inspect", &json!({"id": off})).unwrap()["box"]["x"], 1000.0);
    s.execute("edit.select", &json!({"ids": []})).unwrap();
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    // Hover and click wholly off the slide.
    assert!(matches!(s.hit_test(deckcraft_geom::Point::new(1060.0, 140.0), 3.0), Some(tools::Hit::Shape { id, .. }) if id.0 as u64 == off));
    s.pointer(ev(PointerKind::Down, 1060.0, 140.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 1060.0, 140.0)).unwrap();
    assert_eq!(s.doc().unwrap().selection.shapes.iter().map(|i| i.0 as u64).collect::<Vec<_>>(), vec![off]);
    // Drag it further off, then back onto the slide.
    s.pointer(ev(PointerKind::Down, 1060.0, 140.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 1110.0, 160.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 1110.0, 160.0)).unwrap();
    let v = s.execute("shape.inspect", &json!({"id": off})).unwrap();
    assert_eq!((v["box"]["x"].as_f64(), v["box"]["y"].as_f64()), (Some(1050.0), Some(120.0)));
    s.pointer(ev(PointerKind::Down, 1110.0, 160.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 500.0, 160.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 500.0, 160.0)).unwrap();
    assert_eq!(s.execute("shape.inspect", &json!({"id": off})).unwrap()["box"]["x"], 440.0);
    // The off-slide half of a straddling shape is hit too.
    s.pointer(ev(PointerKind::Down, -30.0, 340.0)).unwrap();
    s.pointer(ev(PointerKind::Up, -30.0, 340.0)).unwrap();
    assert_eq!(s.doc().unwrap().selection.shapes.iter().map(|i| i.0 as u64).collect::<Vec<_>>(), vec![half]);
    // A marquee started on the pasteboard selects what it encloses there.
    s.pointer(ev(PointerKind::Down, -100.0, 250.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 80.0, 420.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 80.0, 420.0)).unwrap();
    assert_eq!(s.doc().unwrap().selection.shapes.iter().map(|i| i.0 as u64).collect::<Vec<_>>(), vec![half]);
}

fn title_text(s: &Session) -> String {
    let sh = &s.doc().unwrap().current_slide().unwrap().shapes[0];
    sh.text.as_ref().map(|t| t.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

/// #101: Korean typed through an input method arrives as preedit updates (ㅎ → 하 → 한 → 한ㄱ →
/// 한그 → 한글) and a commit. The preedit is shown in place and replaced on each update; the
/// committed text is one undo step, and nothing on the way is an undo step of its own.
#[test]
fn ime_composition_replaces_its_preedit_and_commits_as_one_undo_step() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "A "})).unwrap();
    let undo0 = s.doc().unwrap().history.undo.len();
    for pre in ["ㅎ", "하", "한", "한ㄱ", "한그", "한글"] {
        s.compose_text(pre, None).unwrap();
        assert_eq!(title_text(&s), format!("A {pre}"), "the preedit is shown in place");
        let c = s.doc().unwrap().composition.clone().unwrap();
        assert_eq!((c.start, c.len), ((0, 2), pre.chars().count()));
        assert_eq!(s.doc().unwrap().selection.text.as_ref().unwrap().caret, (0, 2 + pre.chars().count()), "caret after the preedit");
        assert_eq!(s.doc().unwrap().history.undo.len(), undo0, "no undo step while composing");
    }
    s.commit_text("한글").unwrap();
    assert_eq!(title_text(&s), "A 한글");
    assert!(s.doc().unwrap().composition.is_none());
    assert_eq!(s.doc().unwrap().history.undo.len(), undo0 + 1, "one undo step for the composition");
    assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Type Text");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(title_text(&s), "A ");
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(title_text(&s), "A 한글");

    // Windows' Korean IME commits a syllable when the next begins: an empty preedit, the commit,
    // then a new preedit.
    s.compose_text("ㄴ", None).unwrap();
    s.compose_text("", None).unwrap();
    s.commit_text("ㄴ").unwrap();
    s.compose_text("ㅁ", None).unwrap();
    assert_eq!(title_text(&s), "A 한글ㄴㅁ");
    s.commit_text("마").unwrap();
    assert_eq!(title_text(&s), "A 한글ㄴ마");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(title_text(&s), "A 한글ㄴ");
}

/// #101: an empty preedit cancels the composition (Escape, or Backspace over its last jamo):
/// the text and the selection it replaced are as they were, with no undo step.
#[test]
fn ime_cancel_restores_the_text_and_selection() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Hello world"})).unwrap();
    s.execute("text.select", &json!({"anchor": [0, 6], "caret": [0, 11]})).unwrap();
    let undo0 = s.doc().unwrap().history.undo.len();
    let rev = s.doc().unwrap().revision;
    s.compose_text("せかい", Some((0, 3))).unwrap();
    assert_eq!(title_text(&s), "Hello せかい", "the preedit replaces the selection");
    assert_eq!(s.doc().unwrap().composition.as_ref().unwrap().active, Some((0, 3)));
    s.compose_text("世界", Some((0, 9))).unwrap();
    assert_eq!(s.doc().unwrap().composition.as_ref().unwrap().active, Some((0, 2)), "clamped to the preedit");
    s.compose_text("", None).unwrap();
    assert_eq!(title_text(&s), "Hello world");
    let t = s.doc().unwrap().selection.text.clone().unwrap();
    assert_eq!((t.anchor, t.caret), ((0, 6), (0, 11)), "the selection is back");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo0);
    assert!(s.doc().unwrap().revision > rev, "redrawn");
    // A commit of nothing without a composition is nothing.
    s.commit_text("").unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), undo0);
    // A commit without a preedit (some input methods send only the result) is one step.
    s.commit_text("世界").unwrap();
    assert_eq!(title_text(&s), "Hello 世界");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo0 + 1);
}

/// #101: anything else settles the composition first: its text stays and is its own undo step.
#[test]
fn ime_composition_is_kept_by_a_key_a_click_or_another_command() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    let undo0 = s.doc().unwrap().history.undo.len();
    s.compose_text("안", None).unwrap();
    s.key("Right", Mods::default()).unwrap();
    assert!(s.doc().unwrap().composition.is_none());
    assert_eq!(title_text(&s), "안");
    assert_eq!(s.doc().unwrap().history.undo.len(), undo0 + 1);

    s.compose_text("녕", None).unwrap();
    s.pointer(PointerEvent { kind: PointerKind::Move, x: 5.0, y: 5.0, mods: Mods::default(), tol: 4.0, pressure: None }).unwrap();
    assert!(s.doc().unwrap().composition.is_some(), "hovering keeps composing");
    s.pointer(PointerEvent { kind: PointerKind::Down, x: 5.0, y: 5.0, mods: Mods::default(), tol: 4.0, pressure: None }).unwrap();
    assert!(s.doc().unwrap().composition.is_none(), "a click settles it");
    assert_eq!(title_text(&s), "안녕");

    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.compose_text("하", None).unwrap();
    s.execute("text.get", &json!({"id": title})).unwrap();
    assert!(s.doc().unwrap().composition.is_some(), "a query doesn't");
    s.execute("text.insert", &json!({"text": "!"})).unwrap();
    assert_eq!(title_text(&s), "안녕하!");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(title_text(&s), "안녕하");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(title_text(&s), "안녕");

    // Text changed under the composition (here behind its back): a new one starts, no panic.
    s.compose_text("ㄱ", None).unwrap();
    s.doc_mut().unwrap().composition.as_mut().unwrap().len = 99;
    s.compose_text("가", None).unwrap();
    assert!(title_text(&s).ends_with('가'));
    // Control characters (an Enter sent as text) never split the paragraph.
    s.commit_text("가\n\r\u{b}").unwrap();
    assert_eq!(s.doc().unwrap().current_slide().unwrap().shapes[0].text.as_ref().unwrap().paragraphs.len(), 1);
}

/// #101: the first preedit on a selected shape starts editing it and replaces its text, as typing
/// does; with nothing selected there is nowhere to compose.
#[test]
fn ime_on_a_selected_shape_replaces_its_text() {
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Old"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.select(|_, sel| {
        sel.text = None;
        sel.shapes = vec![ShapeId(id as u32)];
    })
    .unwrap();
    assert_eq!(s.compose_text("", None).unwrap()["handled"], false, "an empty preedit doesn't start editing");
    s.compose_text("新", None).unwrap();
    s.commit_text("新しい").unwrap();
    let v = s.execute("text.get", &json!({"id": id})).unwrap();
    assert_eq!(v["text"], "新しい");
    s.select(|_, sel| {
        sel.text = None;
        sel.shapes.clear();
    })
    .unwrap();
    assert_eq!(s.compose_text("x", None).unwrap()["handled"], false);
    assert_eq!(s.commit_text("x").unwrap()["handled"], false);
}

/// #101: the composition is underlined in place, under the composed characters, with a thicker
/// line under the part being converted.
#[test]
fn ime_composition_underlines_follow_the_text() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    assert!(cmd::text::composition_underlines(s.doc().unwrap()).is_empty());
    s.compose_text("にほんご", Some((2, 4))).unwrap();
    let u = cmd::text::composition_underlines(s.doc().unwrap());
    let thin: Vec<_> = u.iter().filter(|x| !x.2).collect();
    let thick: Vec<_> = u.iter().filter(|x| x.2).collect();
    assert_eq!((thin.len(), thick.len()), (1, 1), "{u:?}");
    let sh = s.doc().unwrap().current_slide().unwrap().shapes[0].clone();
    let x = cmd::xfrm_of(&s.doc().unwrap().doc, &s.doc().unwrap().selection, &sh);
    for (a, b, _) in &u {
        assert!(b.x > a.x && (a.y - b.y).abs() < 1e-9, "a horizontal line: {a:?} {b:?}");
        assert!(a.x >= x.x - 1.0 && b.x <= x.x + x.w + 1.0 && a.y > x.y && a.y < x.y + x.h, "inside the title box");
    }
    assert!(thick[0].0.x > thin[0].0.x && (thick[0].1.x - thin[0].1.x).abs() < 1e-6, "the converted part is the end");
    s.commit_text("日本語").unwrap();
    assert!(cmd::text::composition_underlines(s.doc().unwrap()).is_empty());
}

/// #101: a composition in a table cell is underlined inside that cell, the same way slide text is.
#[test]
fn ime_composition_in_a_table_cell_is_underlined() {
    let mut s = session();
    let id = s.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [80.0, 90.0, 400.0, 120.0]})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id, "cell": [0, 0]})).unwrap();
    assert!(cmd::text::composition_underlines(s.doc().unwrap()).is_empty());
    let ts = s.doc().unwrap().selection.text.clone().unwrap();
    assert!(cmd::text::caret_line(s.doc().unwrap(), &ts).is_some(), "the caret is in the cell");
    s.compose_text("にほんご", Some((2, 4))).unwrap();
    let u = cmd::text::composition_underlines(s.doc().unwrap());
    let thin: Vec<_> = u.iter().filter(|x| !x.2).collect();
    let thick: Vec<_> = u.iter().filter(|x| x.2).collect();
    assert_eq!((thin.len(), thick.len()), (1, 1), "{u:?}");
    let sh = s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().clone();
    let x = cmd::xfrm_of(&s.doc().unwrap().doc, &s.doc().unwrap().selection, &sh);
    let deckcraft_model::ShapeKind::Table(tb) = &sh.kind else { panic!("not a table") };
    let col = tb.cols[0];
    let row = cmd::table_drawn_heights(&s.doc().unwrap().doc, &s.doc().unwrap().selection, tb)[0];
    for (a, b, _) in &u {
        assert!(b.x > a.x && (a.y - b.y).abs() < 1e-6, "a horizontal line: {a:?} {b:?}");
        assert!(a.x >= x.x - 1.0 && b.x <= x.x + col + 1.0, "inside the first column");
        assert!(a.y > x.y && a.y < x.y + row + 1.0, "inside the first row");
    }
    assert!(thick[0].0.x > thin[0].0.x && (thick[0].1.x - thin[0].1.x).abs() < 1e-6, "the converted part is the end");
    let ts = s.doc().unwrap().selection.text.clone().unwrap();
    let (top, bot) = cmd::text::caret_line(s.doc().unwrap(), &ts).unwrap();
    assert!(top.x >= x.x && top.x <= x.x + col && top.y >= x.y && bot.y <= x.y + row + 1.0, "caret in the cell");
    s.commit_text("日本語").unwrap();
    assert!(cmd::text::composition_underlines(s.doc().unwrap()).is_empty());
    let sh = s.doc().unwrap().shape(deckcraft_model::ShapeId(id as u32)).unwrap().clone();
    let deckcraft_model::ShapeKind::Table(tb) = &sh.kind else { panic!("not a table") };
    assert_eq!(tb.cell(0, 0).map(|c| c.text.text()), Some("日本語".into()));
}

/// File › Info (#76): the active presentation's name, location, counts and properties.
#[test]
fn file_info_reports_the_presentation() {
    let mut s = session();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.go", &json!({"index": 1})).unwrap();
    s.execute("slide.hide", &json!({})).unwrap();
    s.execute("file.properties", &json!({"title": "Quarterly", "author": "Ada"})).unwrap();
    let v = s.execute("file.info", &json!({})).unwrap();
    assert_eq!(v["slides"], 3);
    assert_eq!(v["hiddenSlides"], 1);
    assert_eq!(v["path"], serde_json::Value::Null);
    assert_eq!(v["dirty"], true);
    assert_eq!(v["properties"]["title"], "Quarterly");
    assert_eq!(v["properties"]["author"], "Ada");
    assert!(v["title"].as_str().is_some_and(|t| !t.is_empty()));
    // A query: no undo step, and nothing changes.
    let before = s.doc().unwrap().history.undo.len();
    s.execute("file.info", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), before);
    // Saved to disk, it reports where and how big.
    let dir = std::env::temp_dir().join(format!("deckcraft-info-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Info test.pptx").to_string_lossy().to_string();
    s.execute("file.saveAs", &json!({"path": path})).unwrap();
    let v = s.execute("file.info", &json!({})).unwrap();
    assert_eq!(v["path"], path.as_str());
    assert_eq!(v["format"], "pptx");
    assert_eq!(v["dirty"], false);
    assert!(v["bytes"].as_u64().is_some_and(|b| b > 0));
    let _ = std::fs::remove_dir_all(&dir);
    // Nothing open: an error, not a panic.
    let mut empty = Session::new();
    assert!(empty.execute("file.info", &json!({})).is_err());
}

/// The Print page's Slides box (#76): PowerPoint-style ranges, 1-based.
#[test]
fn slide_ranges_parse_like_powerpoints_print_box() {
    use cmd::file::parse_slide_range as r;
    assert_eq!(r("1-3, 5", 6).unwrap(), vec![0, 1, 2, 4]);
    assert_eq!(r("3-1;2", 6).unwrap(), vec![0, 1, 2], "reversed ranges and repeats");
    assert_eq!(r(" 6 ", 6).unwrap(), vec![5]);
    assert!(r("7", 6).is_err(), "past the last slide");
    assert!(r("0", 6).is_err());
    assert!(r("a-b", 6).is_err());
    assert!(r(" , ", 6).is_err());
    assert!(r("1-99999999999999999999999", 6).is_err(), "hostile numbers are refused");
    // file.export takes the same text for a PDF.
    let mut s = session();
    s.execute("slide.new", &json!({})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let one = cmd::file::pdf_bytes(&doc, &json!({"slides": "2"})).unwrap();
    let both = cmd::file::pdf_bytes(&doc, &json!({"slides": "1-2"})).unwrap();
    assert!(one.len() < both.len());
    assert!(cmd::file::pdf_bytes(&doc, &json!({"slides": "5"})).is_err());
    let opts = cmd::file::pdf_options(&json!({"slides": [0, 7, 1]}), 2).unwrap();
    assert_eq!(opts.slides, Some(vec![0, 1]), "indices past the end are dropped");
}

/// #725: a layout with a small body placeholder ahead of its content placeholder (as Keynote
/// exports "Title & Bullets") still gets the body in the content placeholder (`idx="1"`).
#[test]
fn new_slide_body_goes_to_the_content_placeholder() {
    for before in [true, false] {
        let mut s = session();
        let doc = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc);
        let id = deckcraft_model::ShapeId(doc.alloc_id().expect("id"));
        let lay = std::sync::Arc::make_mut(&mut doc.masters[0])
            .layouts
            .iter_mut()
            .find(|l| l.kind == deckcraft_model::LayoutType::TitleAndContent)
            .unwrap();
        let at = lay.shapes.iter().position(|x| x.ph.as_ref().is_some_and(|p| p.idx == 1)).unwrap();
        let mut small = lay.shapes[at].clone();
        small.id = id;
        small.name = "Subtitle 21".into();
        let ph = small.ph.as_mut().unwrap();
        ph.kind = deckcraft_model::PhType::Body;
        ph.idx = 21;
        ph.size = Some("quarter".into());
        lay.shapes.insert(if before { at } else { at + 1 }, small);

        s.execute("slide.new", &json!({"layout": "titleAndContent", "title": "T", "body": "Line one\nLine two"})).unwrap();
        let doc = &s.doc().unwrap().doc;
        let slide = &doc.slides[doc.slides.len() - 1];
        let text = |idx: u32| {
            let sh = slide.shapes.iter().find(|x| x.ph.as_ref().is_some_and(|p| p.idx == idx)).unwrap();
            sh.text.as_ref().map(|t| t.text()).unwrap_or_default()
        };
        assert_eq!(text(1), "Line one\nLine two", "before = {before}");
        assert_eq!(text(21), "", "before = {before}");
    }
}

#[test]
fn cell_fill_walks_only_the_cells_a_huge_selection_covers() {
    // A selection may reach far past the table; filling it touches the real cells and nothing more.
    let mut s = session();
    let id = s.execute("insert.table", &json!({"rows": 2, "cols": 2, "rect": [100.0, 100.0, 200.0, 40.0]})).unwrap()["id"].clone();
    s.execute("table.selectCells", &json!({"id": id, "from": [1, 1], "to": [1_000_000_000u64, 1_000_000_000u64]})).unwrap();
    s.execute("table.cellFill", &json!({"none": true})).unwrap();
    let st = s.doc().unwrap();
    let sh = st.shapes().iter().find(|sh| json!(sh.id) == id).unwrap();
    let deckcraft_model::ShapeKind::Table(t) = &sh.kind else { panic!("not a table") };
    let fills: Vec<bool> = [(0, 0), (0, 1), (1, 0), (1, 1)].iter().map(|&(r, c)| t.cell(r, c).is_some_and(|x| x.fill.is_some())).collect();
    assert_eq!(fills, [false, false, false, true]);
}

/// Each slide as (title, [(level, bullet text)]).
fn outline_of(p: &deckcraft_model::Presentation) -> Vec<(String, Vec<(u8, String)>)> {
    p.slides
        .iter()
        .map(|s| {
            let body =
                s.shapes.get(1).and_then(|b| b.text.as_ref()).map(|t| t.paragraphs.iter().map(|p| (p.level, p.text())).collect()).unwrap_or_default();
            (s.title(), body)
        })
        .collect()
}

const MD_OUTLINE: &str = "# Intro\n- Point one\n- Point two\n  - Sub point\n\n# Second\n- More\n";

fn md_expected() -> Vec<(String, Vec<(u8, String)>)> {
    vec![
        ("Intro".into(), vec![(0, "Point one".into()), (0, "Point two".into()), (1, "Sub point".into())]),
        ("Second".into(), vec![(0, "More".into())]),
    ]
}

#[test]
fn markdown_outlines_take_headings_as_slides_and_lists_as_bullets() {
    let p = crate::cmd::file::open_presentation("outline.md", MD_OUTLINE.as_bytes()).unwrap();
    assert_eq!(outline_of(&p), md_expected());
    // numbered lists, `*` / `+` bullets, text under a heading, and closing #s
    let p = crate::cmd::file::open_presentation("o.md", b"## Plan ##\n1. First\n2) Second\n* star\n+ plus\nA plain line\n").unwrap();
    assert_eq!(
        outline_of(&p),
        vec![("Plan".into(), vec![(0, "First".into()), (0, "Second".into()), (0, "star".into()), (0, "plus".into()), (0, "A plain line".into())])]
    );
}

#[test]
fn slides_from_a_markdown_outline_file() {
    let mut s = session();
    s.execute("slide.fromOutline", &json!({"text": MD_OUTLINE, "markdown": true})).unwrap();
    let doc = &s.doc().unwrap().doc;
    assert_eq!(outline_of(doc)[1..], md_expected()[..]);
}

#[test]
fn plain_outlines_are_unchanged() {
    // .txt keeps the indentation format, and so does a .md without headings
    let indented = "Intro\n\tPoint one\n\t\tSub point\nSecond\n\tMore\n";
    let want =
        vec![("Intro".to_string(), vec![(0, "Point one".to_string()), (1, "Sub point".to_string())]), ("Second".into(), vec![(0, "More".into())])];
    for name in ["outline.txt", "outline.md"] {
        let p = crate::cmd::file::open_presentation(name, indented.as_bytes()).unwrap();
        assert_eq!(outline_of(&p), want, "{name}");
    }
    let p = crate::cmd::file::open_presentation("outline.txt", MD_OUTLINE.as_bytes()).unwrap();
    assert_eq!(p.slides.first().map(|s| s.title()).as_deref(), Some("# Intro"));
}

fn text_box_xfrm(s: &Session, id: u64) -> deckcraft_geom::Xfrm {
    let st = s.doc().unwrap();
    cmd::xfrm_of(&st.doc, &st.selection, st.shape(deckcraft_model::ShapeId(id as u32)).unwrap())
}

fn right_click(s: &mut Session, x: f64, y: f64) -> serde_json::Value {
    s.pointer(PointerEvent { kind: PointerKind::ContextClick, x, y, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap()
}

/// A blank slide with a rectangle A (100,100 200×100) and a text box B (400,100 200 wide),
/// nothing selected.
fn two_shapes() -> (Session, deckcraft_model::ShapeId, deckcraft_model::ShapeId) {
    let mut s = session();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100.0, 100.0, 200.0, 100.0], "text": "Shape A"})).unwrap()["id"]
        .as_u64()
        .unwrap();
    let b = s.execute("insert.textBox", &json!({"rect": [400.0, 100.0, 200.0, 40.0], "text": "Box B"})).unwrap()["id"].as_u64().unwrap();
    s.select(|_, sel| {
        sel.shapes.clear();
        sel.text = None;
    })
    .unwrap();
    (s, deckcraft_model::ShapeId(a as u32), deckcraft_model::ShapeId(b as u32))
}

#[test]
fn right_click_selects_the_shape_under_the_pointer() {
    let (mut s, a, b) = two_shapes();
    let undo_before = s.doc().unwrap().history.undo.len();
    // An unselected shape: selected alone, no text editing, no drag.
    let r = right_click(&mut s, 150.0, 150.0);
    assert_eq!(r["selected"], json!([a]));
    let st = s.doc().unwrap();
    assert_eq!(st.selection.shapes, vec![a]);
    assert!(st.selection.text.is_none());
    assert!(!s.tool.dragging());
    s.pointer(PointerEvent { kind: PointerKind::Drag, x: 190.0, y: 190.0, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap();
    assert_eq!(text_box_xfrm(&s, a.0 as u64).x, 100.0);
    // A text box, which a left click would start editing: only selected.
    right_click(&mut s, 450.0, 115.0);
    let st = s.doc().unwrap();
    assert_eq!(st.selection.shapes, vec![b]);
    assert!(st.selection.text.is_none());
    // A multiple selection stays when the right click lands on one of its shapes.
    s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
    right_click(&mut s, 450.0, 115.0);
    assert_eq!(s.doc().unwrap().selection.shapes, vec![a, b]);
    // Empty slide: nothing selected (the slide menu).
    let r = right_click(&mut s, 700.0, 450.0);
    assert!(r["hit"].is_null(), "{r}");
    assert!(s.doc().unwrap().selection.shapes.is_empty());
    assert_eq!(s.doc().unwrap().history.undo.len(), undo_before);
}

#[test]
fn right_click_in_edited_text_keeps_the_text_selection() {
    let (mut s, a, b) = two_shapes();
    s.execute("text.edit", &json!({"id": a, "at": [0, 0]})).unwrap();
    s.execute("text.move", &json!({"to": "wordRight", "extend": true})).unwrap();
    let before = s.doc().unwrap().selection.text.clone().unwrap();
    assert!(before.is_range());
    // Anywhere in the box, even away from the selected word.
    let r = right_click(&mut s, 280.0, 185.0);
    assert_eq!(r["editing"], true);
    assert_eq!(s.doc().unwrap().selection.text.as_ref(), Some(&before));
    // The context menu's Copy and Cut then act on the selected text.
    assert_eq!(s.execute("edit.copy", &json!({})).unwrap()["text"].as_str().unwrap().trim(), "Shape");
    s.execute("edit.cut", &json!({})).unwrap();
    assert_eq!(s.execute("text.get", &json!({"id": a})).unwrap()["text"].as_str().unwrap().trim(), "A");
    // A right click on another shape leaves editing and selects that shape.
    right_click(&mut s, 450.0, 115.0);
    let st = s.doc().unwrap();
    assert!(st.selection.text.is_none());
    assert_eq!(st.selection.shapes, vec![b]);
}

mod copy_remap {
    use std::sync::Arc;

    use deckcraft_model::style::PictureFill;
    use deckcraft_model::{Animation, Shape, ShapeId, ShapeKind, Xfrm};
    use serde_json::json;

    use crate::*;

    /// Slide 0 of `s` gets a picture of `bytes`, a box, and a connector from the picture to the box.
    /// Returns (picture, box, connector).
    fn populate(s: &mut Session, bytes: &[u8]) -> (ShapeId, ShapeId, ShapeId) {
        let st = s.doc_mut().unwrap();
        let doc = Arc::make_mut(&mut st.doc);
        let media = doc.add_media("p.png", "image/png", bytes.to_vec()).expect("media");
        let (p, b, c) = (ShapeId(doc.alloc_id().expect("id")), ShapeId(doc.alloc_id().expect("id")), ShapeId(doc.alloc_id().expect("id")));
        let x = Some(Xfrm::new(10.0, 10.0, 50.0, 50.0));
        let slide = Arc::make_mut(&mut doc.slides[0]);
        slide.shapes.push(Shape {
            id: p,
            kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
            xfrm: x,
            ..Default::default()
        });
        slide.shapes.push(Shape { id: b, xfrm: x, ..Default::default() });
        slide.shapes.push(Shape { id: c, kind: ShapeKind::Connector { start: Some((p, 1)), end: Some((b, 3)) }, xfrm: x, ..Default::default() });
        (p, b, c)
    }

    fn select(s: &mut Session, ids: &[ShapeId]) {
        let sel = &mut s.doc_mut().unwrap().selection;
        sel.slides.clear();
        sel.shapes = ids.to_vec();
    }

    fn shape(s: &Session, slide: usize, id: ShapeId) -> Shape {
        let d = s.doc().unwrap();
        d.doc.slides[slide].shapes.iter().find(|x| x.id == id).cloned().unwrap()
    }

    fn picture_bytes(s: &Session, sh: &Shape) -> Vec<u8> {
        let ShapeKind::Picture { fill } = &sh.kind else { panic!("not a picture") };
        s.doc().unwrap().doc.media(fill.media).unwrap().data.as_ref().clone()
    }

    #[test]
    fn paste_into_a_deck_with_a_colliding_media_id_shows_the_copied_picture() {
        let mut a = Session::with_new();
        let mut b = Session::with_new();
        let (pa, ..) = populate(&mut a, b"deck a picture");
        let (pb, ..) = populate(&mut b, b"deck b picture");
        assert_eq!(picture_bytes(&a, &shape(&a, 0, pa)), b"deck a picture");
        select(&mut a, &[pa]);
        a.execute("edit.copy", &json!({})).unwrap();
        b.clipboard = a.clipboard.clone();
        b.doc_mut().unwrap().selection.shapes.clear();
        let r = b.execute("edit.paste", &json!({})).unwrap();
        let new = ShapeId(r["ids"][0].as_u64().unwrap() as u32);
        assert_eq!(picture_bytes(&b, &shape(&b, 0, new)), b"deck a picture");
        // Deck b's own picture is untouched.
        assert_eq!(picture_bytes(&b, &shape(&b, 0, pb)), b"deck b picture");
    }

    #[test]
    fn copied_connectors_follow_copied_ends_and_detach_from_the_rest() {
        let mut s = Session::with_new();
        let (p, b, c) = populate(&mut s, b"x");
        // Both ends copied: the copy connects the copies.
        let r = s.execute("edit.duplicate", &json!({"ids": [p.0, b.0, c.0]})).unwrap();
        let ids: Vec<ShapeId> = r["ids"].as_array().unwrap().iter().map(|v| ShapeId(v.as_u64().unwrap() as u32)).collect();
        let ShapeKind::Connector { start, end } = shape(&s, 0, ids[2]).kind else { panic!() };
        assert_eq!((start, end), (Some((ids[0], 1)), Some((ids[1], 3))));
        // Only the connector copied: it doesn't stay glued to the originals.
        select(&mut s, &[c]);
        s.execute("edit.copy", &json!({})).unwrap();
        s.doc_mut().unwrap().selection.shapes.clear();
        let r = s.execute("edit.paste", &json!({})).unwrap();
        let only = ShapeId(r["ids"][0].as_u64().unwrap() as u32);
        let ShapeKind::Connector { start, end } = shape(&s, 0, only).kind else { panic!() };
        assert_eq!((start, end), (None, None));
        // The original keeps its ends.
        let ShapeKind::Connector { start, .. } = shape(&s, 0, c).kind else { panic!() };
        assert_eq!(start, Some((p, 1)));
    }

    fn triggered(s: &mut Session, target: ShapeId, trigger: ShapeId) {
        let st = s.doc_mut().unwrap();
        let doc = Arc::make_mut(&mut st.doc);
        Arc::make_mut(&mut doc.slides[0]).animations.push(Animation {
            shape: target,
            trigger: Some(trigger),
            effect: "fade".into(),
            ..Default::default()
        });
    }

    fn resolves(s: &Session, slide: usize) -> (ShapeId, ShapeId) {
        let d = s.doc().unwrap();
        let sl = &d.doc.slides[slide];
        let a = &sl.animations[0];
        let t = a.trigger.unwrap();
        assert!(sl.shapes.iter().any(|x| x.id == a.shape) && sl.shapes.iter().any(|x| x.id == t), "{a:?}");
        (a.shape, t)
    }

    #[test]
    fn duplicated_and_pasted_slides_retarget_animations_and_triggers() {
        let mut s = Session::with_new();
        let (p, b, _) = populate(&mut s, b"x");
        triggered(&mut s, p, b);
        s.execute("slide.duplicate", &json!({"index": 0})).unwrap();
        let (t, tr) = resolves(&s, 1);
        assert!(t != p && tr != b);
        // Pasted slide.
        s.execute("edit.copy", &json!({"scope": "slides"})).unwrap();
        s.execute("edit.paste", &json!({"scope": "slides"})).unwrap();
        let n = s.doc().unwrap().doc.slides.len();
        let (t2, tr2) = resolves(&s, n - 1);
        assert!(t2 != t && tr2 != tr);
        // Save and reload keeps them resolving.
        let bytes = crate::cmd::file::save_bytes(&s.doc().unwrap().doc, "deckcraft").unwrap();
        let back = deckcraft_format::load(&bytes).unwrap();
        for sl in &back.slides {
            for a in &sl.animations {
                assert!(sl.shapes.iter().any(|x| x.id == a.shape));
                assert!(a.trigger.is_none_or(|t| sl.shapes.iter().any(|x| x.id == t)));
            }
        }
    }
}

#[test]
fn font_add_refuses_non_fonts_and_rescan_reports_the_catalog() {
    let mut s = session();
    let not_font = crate::cmd::base64_encode(b"definitely not a font file");
    assert!(s.execute("font.add", &json!({"data": not_font})).is_err());
    assert!(s.execute("font.add", &json!({})).is_err());
    let r = s.execute("font.rescan", &json!({})).unwrap();
    assert!(r["families"].as_u64().unwrap() >= 1, "{r}");
}

/// The Print page's paper and orientation reach the PDF options; Letter portrait by default.
#[test]
fn pdf_options_take_paper_and_orientation() {
    let o = cmd::file::pdf_options(&json!({}), 1).unwrap();
    assert_eq!(o.paper, (612.0, 792.0));
    let o = cmd::file::pdf_options(&json!({"paper": "A4", "orientation": "landscape"}), 1).unwrap();
    assert_eq!(o.paper, (841.89, 595.28));
    let o = cmd::file::pdf_options(&json!({"paper": "legal"}), 1).unwrap();
    assert_eq!(o.paper, (612.0, 1008.0));
    assert!(cmd::file::pdf_options(&json!({"paper": "tabloid"}), 1).is_err());
    assert!(cmd::file::pdf_options(&json!({"orientation": "sideways"}), 1).is_err());
}

#[test]
fn shape_line_sets_compound_and_arrow_sizes() {
    // The Format Shape pane's Line page: compound type, arrowheads and their sizes.
    use deckcraft_model::style::Compound;
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "line", "rect": [100, 100, 200, 0]})).unwrap();
    s.execute("shape.line", &json!({"compound": "thickThin", "head": "oval", "tail": "triangle", "tailSize": "lg"})).unwrap();
    let line = |s: &Session| s.doc().unwrap().selected_shapes()[0].line.clone().unwrap();
    let l = line(&s);
    assert_eq!(l.compound, Some(Compound::ThickThin));
    assert_eq!(l.head.as_ref().map(|e| (e.kind.as_str(), e.w.as_str())), Some(("oval", "med")));
    assert_eq!(l.tail.as_ref().map(|e| (e.kind.as_str(), e.w.as_str(), e.len.as_str())), Some(("triangle", "lg", "lg")));
    // Changing the arrow keeps its size; an unknown size is ignored; "none" removes it.
    s.execute("shape.line", &json!({"tail": "stealth", "headSize": "huge"})).unwrap();
    let l = line(&s);
    assert_eq!(l.tail.as_ref().map(|e| (e.kind.as_str(), e.w.as_str())), Some(("stealth", "lg")));
    assert_eq!(l.head.as_ref().map(|e| e.w.as_str()), Some("med"));
    s.execute("shape.line", &json!({"head": "none", "compound": "single"})).unwrap();
    let l = line(&s);
    assert!(l.head.is_none());
    assert_eq!(l.compound, Some(Compound::Single));
}

#[test]
fn shape_effects_props_adjust_shadow_glow_and_reflection() {
    // The Format Shape pane's Effects page: shadow, glow and reflection details.
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    let fx = |s: &Session| s.doc().unwrap().selected_shapes()[0].effects.clone().unwrap_or_default();
    // No shadow yet: the props add an outer one and set it.
    s.execute("shape.effects", &json!({"shadowProps": {"blur": 10, "dist": 6, "dir": 405, "transparency": 0.25, "color": "accent2"}})).unwrap();
    let sw = fx(&s).outer_shadow.unwrap();
    assert_eq!((sw.blur, sw.dist, sw.dir), (10.0, 6.0, 45.0));
    assert!((sw.color.alpha() - 0.75).abs() < 1e-6);
    // A new colour keeps the transparency; an inner shadow is adjusted in place.
    s.execute("shape.effects", &json!({"shadowProps": {"color": "accent3"}})).unwrap();
    assert!((fx(&s).outer_shadow.unwrap().color.alpha() - 0.75).abs() < 1e-6);
    s.execute("shape.effects", &json!({"shadow": "inner"})).unwrap();
    s.execute("shape.effects", &json!({"shadowProps": {"blur": 3}})).unwrap();
    let f = fx(&s);
    assert!(f.outer_shadow.is_none());
    assert_eq!(f.inner_shadow.unwrap().blur, 3.0);
    // Glow: size, colour, transparency; size 0 removes it.
    s.execute("shape.effects", &json!({"glowProps": {"radius": 12, "transparency": 0.5}})).unwrap();
    let g = fx(&s).glow.unwrap();
    assert_eq!(g.radius, 12.0);
    assert!((g.color.alpha() - 0.5).abs() < 1e-6);
    s.execute("shape.effects", &json!({"glowProps": {"radius": 0}})).unwrap();
    assert!(fx(&s).glow.is_none());
    // Reflection: transparency, size, distance, blur; hostile values are clamped or ignored.
    s.execute("shape.effects", &json!({"reflectionProps": {"transparency": 0.3, "size": 2.0, "dist": 5, "blur": "x"}})).unwrap();
    let r = fx(&s).reflection.unwrap();
    assert!((r.start_alpha - 0.7).abs() < 1e-9);
    assert_eq!((r.end_pos, r.dist, r.blur), (1.0, 5.0, 0.5));
}

/// Text Fill › Transparency (#32): the text keeps its colour, own or inherited, and gets an alpha;
/// 0 makes it opaque again, and it is one undo step.
#[test]
fn text_transparency_keeps_the_colour_and_sets_alpha() {
    use deckcraft_model::{ColorTransform, Fill};
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Hi"})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    let fill = |s: &Session| {
        let st = s.doc().unwrap();
        st.shape(id).unwrap().text.as_ref().unwrap().paragraphs[0].runs[0].props.fill.clone()
    };
    // Inherited colour: the run gets the colour it showed, now 25% transparent.
    s.execute("format.textTransparency", &json!({"value": 0.25})).unwrap();
    let Some(Fill::Solid { color }) = fill(&s) else { panic!("solid fill expected: {:?}", fill(&s)) };
    assert!((color.alpha() - 0.75).abs() < 1e-9, "{color:?}");
    // Its own colour stays; the alpha is replaced, not stacked.
    s.execute("format.color", &json!({"color": "#C00000"})).unwrap();
    s.execute("format.textTransparency", &json!({"value": 0.6})).unwrap();
    s.execute("format.textTransparency", &json!({"value": 0.5})).unwrap();
    let Some(Fill::Solid { color }) = fill(&s) else { panic!() };
    assert_eq!(color.base, deckcraft_model::ColorBase::Rgb { rgb: deckcraft_model::Rgba::rgb(0xC0, 0, 0) });
    assert_eq!(color.mods.iter().filter(|m| matches!(m, ColorTransform::Alpha(_))).count(), 1);
    assert!((color.alpha() - 0.5).abs() < 1e-9);
    s.execute("format.textTransparency", &json!({"value": 0})).unwrap();
    let Some(Fill::Solid { color }) = fill(&s) else { panic!() };
    assert!(color.mods.is_empty(), "opaque drops the alpha: {color:?}");
    s.execute("edit.undo", &json!({})).unwrap();
    let Some(Fill::Solid { color }) = fill(&s) else { panic!() };
    assert!((color.alpha() - 0.5).abs() < 1e-9);
    assert!(s.execute("format.textTransparency", &json!({})).is_err());
}

#[test]
fn edit_points_turns_a_preset_into_a_freeform_and_moves_adds_deletes() {
    use deckcraft_model::Geom;
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    let r = s.execute("shape.editPoints", &json!({})).unwrap();
    let id = r["editing"].as_u64().unwrap();
    assert!(matches!(s.doc().unwrap().selected_shapes()[0].geom, Geom::Custom { .. }));
    let pts =
        |s: &mut Session| -> Vec<[f64; 2]> { serde_json::from_value(s.execute("shape.points", &json!({})).unwrap()["paths"][0].clone()).unwrap() };
    let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    let v = pts(&mut s);
    assert_eq!(v.len(), 4, "{v:?}");
    assert!(near(v[0], [100.0, 100.0]) && near(v[2], [300.0, 200.0]));
    // Drag the bottom-right corner out: the box grows to fit, other points stay on the slide.
    s.execute("shape.movePoint", &json!({"index": 2, "x": 350, "y": 260})).unwrap();
    let v = pts(&mut s);
    assert!(near(v[0], [100.0, 100.0]) && near(v[2], [350.0, 260.0]) && near(v[3], [100.0, 200.0]), "{v:?}");
    let x = s.doc().unwrap().selected_shapes()[0].xfrm.unwrap();
    assert!((x.w - 250.0).abs() < 1e-6 && (x.h - 160.0).abs() < 1e-6);
    // Move the first point up-left: the box moves with it.
    s.execute("shape.movePoint", &json!({"index": 0, "x": 80, "y": 90})).unwrap();
    let x = s.doc().unwrap().selected_shapes()[0].xfrm.unwrap();
    assert!((x.x - 80.0).abs() < 1e-6 && (x.y - 90.0).abs() < 1e-6);
    assert!(near(pts(&mut s)[0], [80.0, 90.0]));
    // Add a point in the middle of the closing edge (from the last point back to the first) and
    // of the top edge, then delete one.
    s.execute("shape.addPoint", &json!({"segment": 0})).unwrap();
    assert_eq!(pts(&mut s).len(), 5);
    s.execute("shape.addPoint", &json!({"segment": 1, "t": 0.5})).unwrap();
    let v = pts(&mut s);
    assert_eq!(v.len(), 6);
    assert!(near(v[1], [(80.0 + 300.0) / 2.0, (90.0 + 100.0) / 2.0]), "{v:?}");
    s.execute("shape.deletePoint", &json!({"index": 1})).unwrap();
    s.execute("shape.deletePoint", &json!({"index": 0})).unwrap();
    assert_eq!(pts(&mut s).len(), 4);
    // Bad input is an error, not a panic; an outline keeps two points.
    assert!(s.execute("shape.movePoint", &json!({"index": 99, "x": 1, "y": 1})).is_err());
    assert!(s.execute("shape.movePoint", &json!({"index": 0, "x": f64::NAN, "y": 1})).is_err());
    s.execute("shape.deletePoint", &json!({"index": 0})).unwrap();
    s.execute("shape.deletePoint", &json!({"index": 0})).unwrap();
    assert!(s.execute("shape.deletePoint", &json!({"index": 0})).is_err());
    // Each edit is one undo step.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(pts(&mut s).len(), 3);
    // It saves as custom geometry.
    let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap();
    s.execute("file.openBytes", &json!({"name": "e.pptx", "data": cmd::base64_encode(&bytes)})).unwrap();
    let back = s.doc().unwrap().doc.slides.iter().flat_map(|sl| sl.shapes.iter()).find(|sh| sh.id.0 == id as u32).map(|sh| sh.geom.clone());
    assert!(matches!(back, Some(Geom::Custom { .. })), "{back:?}");
}

#[test]
fn edit_points_with_the_pointer_and_curves() {
    use crate::tools::{Mods, PointerEvent, PointerKind};
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [100, 100, 200, 100]})).unwrap();
    s.execute("shape.editPoints", &json!({"on": true})).unwrap();
    let pts =
        |s: &mut Session| -> Vec<[f64; 2]> { serde_json::from_value(s.execute("shape.points", &json!({})).unwrap()["paths"][0].clone()).unwrap() };
    let v = pts(&mut s);
    assert!(v.len() >= 4, "{v:?}");
    let ev = |kind, x: f64, y: f64, cmd: bool| PointerEvent { kind, x, y, mods: Mods { cmd, ..Default::default() }, ..Default::default() };
    // Drag the first vertex 30 pt left: one undo step.
    let [x0, y0] = v[0];
    s.pointer(ev(PointerKind::Down, x0, y0, false)).unwrap();
    s.pointer(ev(PointerKind::Drag, x0 - 15.0, y0, false)).unwrap();
    s.pointer(ev(PointerKind::Drag, x0 - 30.0, y0, false)).unwrap();
    s.pointer(ev(PointerKind::Up, x0 - 30.0, y0, false)).unwrap();
    let w = pts(&mut s);
    assert!((w[0][0] - (x0 - 30.0)).abs() < 1e-3 && (w[0][1] - y0).abs() < 1e-3, "{w:?}");
    assert!(w[1..].iter().zip(&v[1..]).all(|(a, b)| (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3));
    s.execute("edit.undo", &json!({})).unwrap();
    assert!((pts(&mut s)[0][0] - x0).abs() < 1e-3);
    // Ctrl-click a vertex deletes it.
    let n = pts(&mut s).len();
    let [x1, y1] = pts(&mut s)[1];
    s.pointer(ev(PointerKind::Down, x1, y1, true)).unwrap();
    s.pointer(ev(PointerKind::Up, x1, y1, true)).unwrap();
    assert_eq!(pts(&mut s).len(), n - 1);
    // Escape leaves Edit Points; then a click on empty slide doesn't touch the shape.
    s.key("Escape", Mods::default()).unwrap();
    assert!(s.tool.edit_points.is_none());
}

#[test]
fn edit_points_path_parse_and_write() {
    use crate::cmd::points::{Seg, parse, vertices, write};
    let segs = parse("M 0 0 L 10 0 C 10 5 5 10 0 10 Z junk L");
    // The closing edge is made explicit; its end is not a vertex of its own.
    assert_eq!(segs.len(), 5);
    assert!(matches!(segs[3], Seg::L(_)));
    assert_eq!(vertices(&segs), vec![0, 1, 2]);
    assert_eq!(write(&parse(&write(&segs))), write(&segs));
    assert!(parse("C 1 2 3").is_empty());
}

#[test]
fn edit_points_types_and_handles() {
    use crate::tools::{Mods, PointerEvent, PointerKind};
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    s.execute("shape.editPoints", &json!({"on": true})).unwrap();
    let q = |s: &mut Session| s.execute("shape.points", &json!({})).unwrap();
    assert_eq!(q(&mut s)["types"][0][1], "corner");
    // A rectangle's corner has no handles until it is a smooth point.
    let id = deckcraft_model::ShapeId(q(&mut s)["id"].as_u64().unwrap() as u32);
    let handles = |s: &Session| {
        let st = s.doc().unwrap();
        crate::cmd::points::slide_handles(&st.doc, &st.selection, st.shape(id).unwrap(), 0, 1).unwrap()
    };
    assert_eq!((handles(&s).0, handles(&s).1), (None, None));
    assert!(s.execute("shape.moveHandle", &json!({"index": 1, "side": "out", "x": 0, "y": 0})).is_err());
    s.execute("shape.pointType", &json!({"index": 1, "type": "smooth"})).unwrap();
    assert_eq!(q(&mut s)["types"][0][1], "smooth");
    let (hi, ho, v) = handles(&s);
    let (hi, ho) = (hi.unwrap(), ho.unwrap());
    assert!(((hi - v).hypot() - (ho - v).hypot()).abs() < 1e-6);
    assert!(((hi - v).normalize() + (ho - v).normalize()).hypot() < 1e-6);
    // Dragging one handle of a smooth point mirrors the other.
    let to = v + deckcraft_geom::Vec2::new(40.0, 10.0);
    s.execute("shape.moveHandle", &json!({"index": 1, "side": "out", "x": to.x, "y": to.y})).unwrap();
    let (hi, ho, v) = handles(&s);
    assert!((ho.unwrap() - to).hypot() < 1e-6 || q(&mut s)["types"][0][1] == "smooth");
    assert!(((hi.unwrap() - v) + (ho.unwrap() - v)).hypot() < 1e-6);
    // Straight: in line, lengths kept; corner: independent.
    s.execute("shape.pointType", &json!({"index": 1, "type": "straight"})).unwrap();
    let (_, ho, v) = handles(&s);
    let far = v + (ho.unwrap() - v) * 3.0;
    s.execute("shape.moveHandle", &json!({"index": 1, "side": "out", "x": far.x, "y": far.y, "type": "straight"})).unwrap();
    assert_eq!(q(&mut s)["types"][0][1], "straight");
    s.execute("shape.pointType", &json!({"index": 1, "type": "corner"})).unwrap();
    let (hi0, ho, v) = handles(&s);
    let off = v + deckcraft_geom::Vec2::new(0.0, -30.0);
    let _ = ho;
    // (The pointer passes the type last chosen for the vertex, as here.)
    s.execute("shape.moveHandle", &json!({"index": 1, "side": "out", "x": off.x, "y": off.y, "type": "corner"})).unwrap();
    let (hi1, _, v1) = handles(&s);
    assert!(((hi1.unwrap() - v1) - (hi0.unwrap() - v)).hypot() < 1e-6);
    assert_eq!(q(&mut s)["types"][0][1], "corner");
    assert!(s.execute("shape.pointType", &json!({"index": 1, "type": "wobbly"})).is_err());
    // With the pointer: click a vertex to show its handles, drag one (one undo step).
    s.execute("shape.pointType", &json!({"index": 2, "type": "smooth"})).unwrap();
    let st = s.doc().unwrap();
    let (_, ho, v) = crate::cmd::points::slide_handles(&st.doc, &st.selection, st.shape(id).unwrap(), 0, 2).unwrap();
    let ho = ho.unwrap();
    let ev = |kind, x: f64, y: f64| PointerEvent { kind, x, y, mods: Mods::default(), ..Default::default() };
    s.pointer(ev(PointerKind::Down, v.x, v.y)).unwrap();
    s.pointer(ev(PointerKind::Up, v.x, v.y)).unwrap();
    assert_eq!(s.tool.edit_vertex, Some((0, 2)));
    s.pointer(ev(PointerKind::Down, ho.x, ho.y)).unwrap();
    s.pointer(ev(PointerKind::Drag, ho.x + 20.0, ho.y + 20.0)).unwrap();
    s.pointer(ev(PointerKind::Up, ho.x + 20.0, ho.y + 20.0)).unwrap();
    let st = s.doc().unwrap();
    let (_, ho2, _) = crate::cmd::points::slide_handles(&st.doc, &st.selection, st.shape(id).unwrap(), 0, 2).unwrap();
    assert!((ho2.unwrap() - (ho + deckcraft_geom::Vec2::new(20.0, 20.0))).hypot() < 1e-3, "{ho2:?}");
    s.execute("edit.undo", &json!({})).unwrap();
    let st = s.doc().unwrap();
    let (_, ho3, _) = crate::cmd::points::slide_handles(&st.doc, &st.selection, st.shape(id).unwrap(), 0, 2).unwrap();
    assert!((ho3.unwrap() - ho).hypot() < 1e-3);
}

/// #392: a `.deckcraft` whose embedded picture is damaged opens as an untitled "(Repaired)"
/// copy with a message, so Save can't overwrite the original with the empty media.
#[test]
fn damaged_native_file_opens_as_repaired_copy() {
    let mut p = deckcraft_model::Presentation::default();
    p.add_media("a.png", "image/png", b"PICTURE-BYTES-0123456789".to_vec());
    let good = deckcraft_format::save(&p).unwrap();
    // PNGs are stored as they are, so flipping a byte breaks the entry's CRC.
    let mut bad = good.clone();
    let at = bad.windows(13).position(|w| w == b"PICTURE-BYTES").unwrap();
    bad[at] ^= 0xff;
    let dir = std::env::temp_dir().join(format!("deckcraft-392-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes, damaged) in [("good.deckcraft", &good, false), ("bad.deckcraft", &bad, true)] {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let mut s = Session::new();
        s.ui_requests.clear();
        let r = s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
        let st = s.doc().unwrap();
        assert_eq!(r["damagedMedia"].as_array().map(|a| a.len()), Some(usize::from(damaged)), "{r}");
        assert_eq!(st.path.is_none(), damaged, "{name}");
        assert_eq!(st.title.ends_with("(Repaired)"), damaged);
        assert_eq!(s.ui_requests.iter().any(|u| matches!(u, crate::UiRequest::Message { .. })), damaged);
        // Bytes route too.
        let r = s.execute("file.openBytes", &json!({"name": name, "data": crate::cmd::base64_encode(bytes)})).unwrap();
        assert_eq!(r["damagedMedia"].as_array().map(|a| a.len()), Some(usize::from(damaged)));
    }
    assert_eq!(std::fs::read(dir.join("bad.deckcraft")).unwrap(), bad, "original untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn motion_path_presets_have_paths_ids_and_round_trip() {
    use deckcraft_model::anim::{ANIMATIONS, AnimClass, preset_path};
    let paths: Vec<_> = ANIMATIONS.iter().filter(|a| a.2 == AnimClass::Path).collect();
    // Preset ids identify the effect when PowerPoint's file is read back: no two share one.
    let mut ids: Vec<u32> = paths.iter().map(|a| a.3).collect();
    ids.sort_unstable();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate motion-path preset ids");
    assert!(n >= 40, "{n} motion paths");
    let mut s = session();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 50, 50]})).unwrap()["id"].clone();
    s.execute("animation.set", &json!({"ids": [id], "effect": "heart", "class": "path"})).unwrap();
    let a = s.doc().unwrap().doc.slides[s.doc().unwrap().selection.slide].animations[0].clone();
    assert_eq!(a.path, preset_path("heart"));
    let p = s.doc().unwrap().doc.as_ref().clone();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    let b = back.slides.iter().flat_map(|sl| &sl.animations).next().unwrap();
    assert_eq!((b.effect.as_str(), b.class), ("heart", AnimClass::Path));
    assert!(b.path.as_deref().is_some_and(|p| p.contains('L')));
}

/// Bullets and Numbering dialog: numbering takes a colour and a size like bullets do, and sizes
/// stay within PowerPoint's 25–400 %.
#[test]
fn numbering_takes_color_and_size() {
    let mut s = Session::with_new();
    s.execute("insert.textBox", &json!({"rect": [10, 10, 300, 100], "text": "One\nTwo"})).unwrap();
    s.execute("format.numbering", &json!({"on": true, "scheme": "romanUcPeriod", "start": 3, "color": "accent2", "size": 150})).unwrap();
    let para = |s: &Session| {
        let st = s.doc().unwrap();
        let id = st.selection.shapes[0];
        st.shape(id).unwrap().text.as_ref().unwrap().paragraphs[0].props.clone()
    };
    let pp = para(&s);
    assert!(matches!(&pp.bullet, Some(deckcraft_model::text::Bullet::AutoNum { scheme, start_at: 3 }) if scheme == "romanUcPeriod"));
    assert_eq!(pp.bullet_size, Some(1.5));
    assert_eq!(pp.bullet_color, Some(deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent2)));
    s.execute("format.bullets", &json!({"on": true, "char": "★", "size": 9000})).unwrap();
    assert_eq!(para(&s).bullet_size, Some(4.0));
}

#[test]
fn paste_special_keeps_source_look_picture_or_text() {
    use deckcraft_model::style::{ColorBase, Fill};
    let mut s = session();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Q3 plan"})).unwrap();
    s.execute("shape.fill", &json!({"color": "accent1"})).unwrap();
    let old_accent = {
        let d = s.doc().unwrap();
        let sl = d.current_slide().unwrap();
        d.doc.master_for(sl).unwrap().0.scheme().get(deckcraft_color::SchemeSlot::Accent1)
    };
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("design.theme", &json!({"name": "Ember"})).unwrap();
    {
        let d = s.doc().unwrap();
        let sl = d.current_slide().unwrap();
        assert_ne!(d.doc.master_for(sl).unwrap().0.scheme().get(deckcraft_color::SchemeSlot::Accent1), old_accent);
    }
    let fill_of = |s: &Session, id: &Value| {
        let sid = deckcraft_model::ShapeId(id.as_u64().unwrap() as u32);
        s.doc().unwrap().shape(sid).unwrap().fill.clone()
    };

    let dest = s.execute("edit.pasteSpecial", &json!({"as": "destinationTheme"})).unwrap();
    assert!(matches!(fill_of(&s, &dest["ids"][0]), Some(Fill::Solid { color }) if matches!(color.base, ColorBase::Scheme { .. })));

    let keep = s.execute("edit.pasteSpecial", &json!({"as": "keepSource"})).unwrap();
    match fill_of(&s, &keep["ids"][0]) {
        Some(Fill::Solid { color }) => assert_eq!(color.base, ColorBase::Rgb { rgb: old_accent }),
        f => panic!("{f:?}"),
    }
    // The clipboard itself still holds the theme-following shape.
    assert!(matches!(&s.clipboard.shapes[0].fill, Some(Fill::Solid { color }) if matches!(color.base, ColorBase::Scheme { .. })));

    let pic = s.execute("edit.pasteSpecial", &json!({"as": "picture"})).unwrap();
    let pid = deckcraft_model::ShapeId(pic["id"].as_u64().or_else(|| pic["ids"][0].as_u64()).unwrap() as u32);
    let d = s.doc().unwrap();
    let sh = d.shape(pid).unwrap();
    let deckcraft_model::ShapeKind::Picture { fill } = &sh.kind else { panic!("{:?}", sh.kind) };
    assert_eq!(d.doc.media(fill.media).unwrap().content_type, "image/png");
    let x = sh.xfrm.unwrap();
    assert!((x.w - 216.0).abs() < 1.0 && (x.h - 116.0).abs() < 1.0, "{x:?}");

    let n = s.doc().unwrap().current_slide().unwrap().shapes.len();
    s.execute("edit.pasteSpecial", &json!({"as": "text"})).unwrap();
    let d = s.doc().unwrap();
    let last = d.current_slide().unwrap().shapes.last().unwrap();
    assert_eq!(d.current_slide().unwrap().shapes.len(), n + 1);
    assert_eq!(last.text.as_ref().unwrap().text(), "Q3 plan");
    assert!(s.execute("edit.pasteSpecial", &json!({"as": "bogus"})).is_err());
}

/// Image export keeps the whole slide: the scale is lowered so the picture fits the renderer's
/// largest side (past it the renderer clips, and big exports came out cropped).
#[test]
fn image_export_scale_fits_the_largest_picture() {
    let mut s = Session::with_new();
    let doc = s.doc().unwrap().doc.clone();
    assert_eq!(crate::cmd::file::image_scale(&doc, 2.0), 2.0);
    assert_eq!(crate::cmd::file::image_scale(&doc, f64::NAN), 1.0);
    let max = f64::from(deckcraft_render::MAX_SIDE);
    assert_eq!(crate::cmd::file::image_scale(&doc, 16.0), 16.0, "16× of 960 pt fits");
    s.execute("design.slideSize", &json!({"w": 4032, "h": 72})).unwrap();
    let doc = s.doc().unwrap().doc.clone();
    let k = crate::cmd::file::image_scale(&doc, 4.0);
    assert!(k * 4032.0 <= max + 1e-6 && k * 4032.0 > max - 1.0, "{k}");
}

/// Exporting every slide as images: when one file can't be written, the error says which
/// slides already were.
#[test]
fn a_failed_all_slides_export_says_what_was_written() {
    let dir = std::env::temp_dir().join(format!("deckcraft-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("deck2.png")).unwrap();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    let e = s.execute("file.export", &json!({"path": dir.join("deck.png").to_string_lossy(), "all": true, "scale": 0.1})).unwrap_err().to_string();
    assert!(e.contains("slides 1–1 were written"), "{e}");
    assert!(dir.join("deck1.png").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Undo keeps a bounded amount of memory: deck-wide edits (each step holds a copy of every
/// slide) are dropped oldest first once the steps pass the byte budget, while ordinary small
/// edits still get the full step count.
#[test]
fn undo_history_is_capped_by_memory_as_well_as_steps() {
    let mut s = Session::with_new();
    for _ in 0..40 {
        s.execute("slide.new", &json!({})).unwrap();
    }
    let one_copy = crate::undo_cost(&s.doc().unwrap().doc, &Presentation::default());
    assert!(one_copy > 0);
    s.doc_mut().unwrap().history = History { limit: 500, byte_limit: one_copy * 10, ..Default::default() };
    for i in 0..30 {
        // Resizing the slides rescales every shape on every slide: a new copy of each.
        let w = if i % 2 == 0 { 720 } else { 960 };
        s.execute("design.slideSize", &json!({"w": w, "h": 540})).unwrap();
    }
    let n = s.doc().unwrap().history.undo.len();
    assert!((5..=11).contains(&n), "{n} deck-wide steps kept");
    // Still undoable, newest first.
    s.execute("edit.undo", &json!({})).unwrap();
    // Small edits: each keeps one slide's copy, so many fit.
    let mut s = Session::with_new();
    s.doc_mut().unwrap().history.byte_limit = one_copy * 10;
    for _ in 0..120 {
        s.execute("slide.new", &json!({})).unwrap();
    }
    assert_eq!(s.doc().unwrap().history.undo.len(), 120);
}

#[test]
fn picture_styles_replace_each_other_in_one_step() {
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf,
        0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60,
        0x82,
    ];
    let mut s = session();
    let data = cmd::base64_encode(PNG);
    let id = s.execute("insert.picture", &json!({"data": data, "name": "a.png", "rect": [100.0, 100.0, 200.0, 150.0]})).unwrap()["id"].clone();
    let pic = |s: &Session| s.doc().unwrap().shapes().iter().find(|sh| json!(sh.id) == id).unwrap().clone();
    assert_eq!(s.execute("picture.styles", &json!({})).unwrap().as_array().unwrap().len(), 24);

    s.execute("picture.style", &json!({"ids": [id], "style": "reflectedRoundedRectangle"})).unwrap();
    let p = pic(&s);
    assert_eq!(p.geom.preset_name(), Some("roundRect"));
    assert!(p.effects.as_ref().unwrap().reflection.is_some());

    // The next style replaces the whole look: no leftover reflection.
    s.execute("picture.style", &json!({"ids": [id], "style": "simpleFrameWhite"})).unwrap();
    let p = pic(&s);
    assert_eq!(p.geom.preset_name(), Some("rect"));
    let e = p.effects.as_ref().unwrap();
    assert!(e.reflection.is_none() && e.outer_shadow.is_some());
    assert_eq!(p.line.as_ref().and_then(|l| l.width), Some(7.0));

    s.execute("picture.style", &json!({"ids": [id], "style": "softEdgeOval"})).unwrap();
    let p = pic(&s);
    assert_eq!(p.geom.preset_name(), Some("ellipse"));
    assert_eq!(p.effects.as_ref().unwrap().soft_edge, Some(12.0));
    assert!(p.line.as_ref().is_some_and(|l| l.fill == Some(deckcraft_model::Fill::None)));

    // Survives PPTX.
    let doc = s.doc().unwrap().doc.as_ref().clone();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&doc).unwrap()).unwrap();
    let bp = back.slides.iter().flat_map(|sl| &sl.shapes).find(|sh| matches!(sh.kind, deckcraft_model::ShapeKind::Picture { .. })).unwrap();
    assert_eq!(bp.geom.preset_name(), Some("ellipse"));
    assert_eq!(bp.effects.as_ref().and_then(|e| e.soft_edge), Some(12.0));

    // One undo step per style; none clears; unknown is refused.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(pic(&s).geom.preset_name(), Some("rect"));
    s.execute("picture.style", &json!({"ids": [id], "style": "none"})).unwrap();
    assert!(pic(&s).effects.is_none() && pic(&s).line.is_none());
    assert!(s.execute("picture.style", &json!({"ids": [id], "style": "sparkly"})).is_err());
}

#[test]
fn picture_recolor_and_transparent_color() {
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf,
        0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60,
        0x82,
    ];
    let mut s = session();
    let id = s.execute("insert.picture", &json!({"data": cmd::base64_encode(PNG), "name": "a.png"})).unwrap()["id"].clone();
    let adj = |s: &Session| match &s.doc().unwrap().shapes().iter().find(|sh| json!(sh.id) == id).unwrap().kind {
        deckcraft_model::ShapeKind::Picture { fill } => fill.adjust.clone(),
        _ => panic!("not a picture"),
    };
    assert_eq!(s.execute("picture.recolors", &json!({})).unwrap().as_array().unwrap().len(), 21);
    s.execute("picture.recolor", &json!({"ids": [id], "preset": "bw50"})).unwrap();
    assert_eq!(adj(&s).bilevel, Some(0.5));
    // A new preset replaces the old one.
    s.execute("picture.recolor", &json!({"ids": [id], "preset": "accent2Light"})).unwrap();
    let a = adj(&s);
    assert!(a.bilevel.is_none());
    assert!(matches!(&a.duotone, Some((d, _)) if *d == deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent2)));
    s.execute("picture.recolor", &json!({"ids": [id], "preset": "washout"})).unwrap();
    let a = adj(&s);
    assert!(a.duotone.is_none() && a.brightness == 0.7 && a.contrast == -0.7);
    s.execute("picture.recolor", &json!({"ids": [id], "preset": "grayscale"})).unwrap();
    let a = adj(&s);
    assert!(a.grayscale && a.brightness == 0.0, "washout undone by the next preset");
    assert!(s.execute("picture.recolor", &json!({"ids": [id], "preset": "plaid"})).is_err());

    s.execute("picture.transparentColor", &json!({"ids": [id], "color": "#FFFFFF"})).unwrap();
    assert_eq!(adj(&s).clear_color, Some(deckcraft_color::Rgba::rgb(255, 255, 255)));
    // Both survive PPTX.
    let doc = s.doc().unwrap().doc.as_ref().clone();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&doc).unwrap()).unwrap();
    let ba = back.slides.iter().flat_map(|sl| &sl.shapes).find_map(|sh| match &sh.kind {
        deckcraft_model::ShapeKind::Picture { fill } => Some(fill.adjust.clone()),
        _ => None,
    });
    let ba = ba.unwrap();
    assert!(ba.grayscale);
    assert_eq!(ba.clear_color, Some(deckcraft_color::Rgba::rgb(255, 255, 255)));
    s.execute("picture.transparentColor", &json!({"ids": [id], "none": true})).unwrap();
    assert!(adj(&s).clear_color.is_none());
    s.execute("picture.recolor", &json!({"ids": [id], "preset": "none"})).unwrap();
    assert_eq!(adj(&s), deckcraft_model::style::PictureAdjust::default());
}

#[test]
fn transparent_color_at_a_point_takes_the_pictures_own_pixel_there() {
    // Red on the left half, blue on the right.
    let img = image::RgbaImage::from_fn(4, 2, |x, _| if x < 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
    let mut png = vec![];
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut s = session();
    let id =
        s.execute("insert.picture", &json!({"data": cmd::base64_encode(&png), "name": "a.png", "rect": [100, 100, 400, 200]})).unwrap()["id"].clone();
    let clear = |s: &Session| match &s.doc().unwrap().shapes().iter().find(|sh| json!(sh.id) == id).unwrap().kind {
        deckcraft_model::ShapeKind::Picture { fill } => fill.adjust.clear_color,
        _ => panic!("not a picture"),
    };
    let red = deckcraft_color::Rgba::rgb(255, 0, 0);
    let blue = deckcraft_color::Rgba::rgb(0, 0, 255);
    s.execute("picture.transparentColor", &json!({"ids": [id], "at": [450, 200]})).unwrap();
    assert_eq!(clear(&s), Some(blue));
    // The picture's own colour, not the one drawn after a correction brightened it.
    s.execute("picture.adjust", &json!({"ids": [id], "brightness": 0.4})).unwrap();
    s.execute("picture.transparentColor", &json!({"ids": [id], "at": [150, 200]})).unwrap();
    assert_eq!(clear(&s), Some(red));
    // Flipped, the blue half is drawn on the left.
    s.execute("shape.flip", &json!({"ids": [id], "axis": "horizontal"})).unwrap();
    s.execute("picture.transparentColor", &json!({"ids": [id], "at": [150, 200]})).unwrap();
    assert_eq!(clear(&s), Some(blue));
    // Beside the picture there is no colour to take, and nothing changes.
    assert!(s.execute("picture.transparentColor", &json!({"ids": [id], "at": [50, 50]})).is_err());
    assert_eq!(clear(&s), Some(blue));
    // A spot that is already transparent hides a colour (black here, as in most PNGs): taking it
    // would clear the black drawn elsewhere in the picture.
    let img = image::RgbaImage::from_fn(4, 2, |x, _| if x < 2 { image::Rgba([0, 0, 0, 0]) } else { image::Rgba([0, 0, 0, 255]) });
    let mut png = vec![];
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let logo =
        s.execute("insert.picture", &json!({"data": cmd::base64_encode(&png), "name": "b.png", "rect": [100, 400, 400, 200]})).unwrap()["id"].clone();
    assert!(s.execute("picture.transparentColor", &json!({"ids": [logo], "at": [150, 500]})).is_err());
    s.execute("picture.transparentColor", &json!({"ids": [logo], "at": [450, 500]})).unwrap();
}

/// A picture of `n` one-pixel columns whose red channel is 20 * column, inserted at `rect`.
fn ramp_picture(s: &mut Session, n: u32, rect: [u32; 4]) -> serde_json::Value {
    let img = image::RgbaImage::from_fn(n, 1, |x, _| image::Rgba([(20 * x) as u8, 0, 0, 255]));
    let mut png = vec![];
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    s.execute("insert.picture", &json!({"data": cmd::base64_encode(&png), "name": "r.png", "rect": rect})).unwrap()["id"].clone()
}

fn clear_color_of(s: &Session, id: &serde_json::Value) -> Option<deckcraft_color::Rgba> {
    fn find(shapes: &[deckcraft_model::Shape], id: &serde_json::Value) -> Option<deckcraft_model::Shape> {
        shapes.iter().find_map(|sh| {
            if json!(sh.id) == *id {
                return Some(sh.clone());
            }
            match &sh.kind {
                deckcraft_model::ShapeKind::Group { children, .. } => find(children, id),
                _ => None,
            }
        })
    }
    match find(s.doc().unwrap().shapes(), id).unwrap().kind {
        deckcraft_model::ShapeKind::Picture { fill } => fill.adjust.clear_color,
        _ => panic!("not a picture"),
    }
}

#[test]
fn transparent_color_at_a_point_follows_the_pictures_3d_camera() {
    let mut s = session();
    let id = ramp_picture(&mut s, 10, [100, 100, 200, 100]);
    // Turned 60 degrees about the vertical axis the picture is drawn about half as wide, centred
    // where it was: 149..251 instead of 100..300.
    s.execute("shape.effects", &json!({"ids": [id], "threeD": {"camera": "orthographicFront", "rotX": 60}})).unwrap();
    // 175 is a quarter of the way across the drawn picture: column 2, not column 3 (the 75/200
    // of the unturned box).
    s.execute("picture.transparentColor", &json!({"ids": [id], "at": [175, 150]})).unwrap();
    assert_eq!(clear_color_of(&s, &id), Some(deckcraft_color::Rgba::rgb(40, 0, 0)));
    // Inside the unturned box but outside the drawn picture: nothing is under the pointer.
    assert!(s.execute("picture.transparentColor", &json!({"ids": [id], "at": [120, 150]})).is_err());
}

#[test]
fn transparent_color_at_a_point_finds_a_grouped_picture_without_its_own_box() {
    let mut s = session();
    let pic = ramp_picture(&mut s, 10, [100, 100, 200, 100]);
    let other = s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 300, 50, 50]})).unwrap()["id"].clone();
    s.execute("edit.select", &json!({"ids": [pic, other]})).unwrap();
    s.execute("arrange.group", &json!({})).unwrap();
    // A grouped picture that keeps no box of its own is drawn where the default box puts it.
    let sz = s.doc().unwrap().doc.slide_size;
    s.edit(|doc, sel| {
        fn clear(shapes: &mut [deckcraft_model::Shape], id: &serde_json::Value) {
            for sh in shapes {
                if json!(sh.id) == *id {
                    sh.xfrm = None;
                }
                if let deckcraft_model::ShapeKind::Group { children, .. } = &mut sh.kind {
                    clear(children, id);
                }
            }
        }
        clear(crate::shapes_mut(doc, sel).unwrap(), &pic);
        Ok(())
    })
    .unwrap();
    // Left tenth of the default box (10%, 10%, 80% x 20% of the slide): column 0.
    let at = [sz.width * 0.1 + 1.0, sz.height * 0.2];
    s.execute("picture.transparentColor", &json!({"ids": [pic], "at": at})).unwrap();
    assert_eq!(clear_color_of(&s, &pic), Some(deckcraft_color::Rgba::rgb(0, 0, 0)));
    let at = [sz.width * 0.9 - 1.0, sz.height * 0.2];
    s.execute("picture.transparentColor", &json!({"ids": [pic], "at": at})).unwrap();
    assert_eq!(clear_color_of(&s, &pic), Some(deckcraft_color::Rgba::rgb(180, 0, 0)));
}

#[test]
fn transparent_color_at_a_point_takes_the_topmost_selected_picture() {
    let mut s = session();
    // `under` is inserted first (drawn below), `over` after it (drawn on top); they overlap in x 150..300.
    let under = ramp_picture(&mut s, 10, [100, 100, 200, 100]);
    let over = ramp_picture(&mut s, 10, [150, 100, 200, 100]);
    // Selected bottom first: selection order is the opposite of the stacking order.
    s.execute("edit.select", &json!({"ids": [under, over]})).unwrap();
    // x = 250 is column 7 of `under` (140) and column 5 of `over` (100): the click is on `over`.
    s.execute("picture.transparentColor", &json!({"at": [250, 150]})).unwrap();
    assert_eq!(clear_color_of(&s, &over), Some(deckcraft_color::Rgba::rgb(100, 0, 0)));
    assert_eq!(clear_color_of(&s, &under), None);
    // Where `over` is not, the picture below is still found.
    s.execute("picture.transparentColor", &json!({"at": [120, 150]})).unwrap();
    assert_eq!(clear_color_of(&s, &under), Some(deckcraft_color::Rgba::rgb(20, 0, 0)));
}

#[test]
fn select_all_slides_scope() {
    let mut s = session();
    // One slide: selects it, no panic.
    let r = s.execute("edit.selectAll", &json!({"scope": "slides"})).unwrap();
    assert_eq!(r["selected"].as_array().map(Vec::len), Some(1));
    for _ in 0..3 {
        s.execute("slide.new", &json!({})).unwrap();
    }
    s.execute("slide.hide", &json!({"index": 1})).unwrap();
    s.execute("slide.go", &json!({"index": 1})).unwrap();
    let before = s.doc().unwrap().selection.slide;
    s.execute("edit.selectAll", &json!({"scope": "slides"})).unwrap();
    let st = s.doc().unwrap();
    let all: Vec<_> = st.doc.slides.iter().map(|x| x.id).collect();
    assert_eq!(st.selection.slides, all);
    assert_eq!(st.selection.slide, before, "the current slide must not jump");
    assert!(st.selection.shapes.is_empty() && st.selection.text.is_none());
    assert_eq!(st.doc.slides.len(), 4);
}

#[test]
fn select_all_slides_leaves_shape_select_all_alone() {
    let mut s = session();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("slide.go", &json!({"index": 0})).unwrap();
    // No scope: still selects shapes on the current slide.
    s.execute("edit.selectAll", &json!({})).unwrap();
    assert!(!s.doc().unwrap().selection.shapes.is_empty());
    assert!(s.doc().unwrap().selection.slides.len() <= 1);
}

#[test]
fn select_all_slides_bad_scope_is_an_error() {
    let mut s = session();
    assert!(s.execute("edit.selectAll", &json!({"scope": "everything"})).is_err());
    assert!(s.execute("edit.selectAll", &json!({"scope": 3})).is_err());
}

#[test]
fn table_borders_margins_and_cell_alignment() {
    let mut s = session();
    let id = s.execute("insert.table", &json!({"rows": 3, "cols": 3, "rect": [100.0, 100.0, 300.0, 120.0]})).unwrap()["id"].clone();
    let table = |s: &Session| {
        let st = s.doc().unwrap();
        let sh = st.shapes().iter().find(|sh| json!(sh.id) == id).unwrap().clone();
        let deckcraft_model::ShapeKind::Table(t) = sh.kind else { panic!("not a table") };
        t
    };
    // Outside borders round the top-left 2×2 block; neighbours get the facing side too.
    let block = json!([[0, 0], [0, 1], [1, 0], [1, 1]]);
    s.execute("table.borders", &json!({"id": id, "which": "outside", "cells": block, "color": "#FF0000", "width": 2.0, "dash": "dash"})).unwrap();
    let t = table(&s);
    let w = |r: usize, c: usize, side: usize| t.rows[r].cells[c].borders[side].as_ref().and_then(|l| l.width);
    assert_eq!(w(0, 0, 0), Some(2.0));
    assert_eq!(w(0, 0, 2), Some(2.0));
    assert_eq!(w(0, 0, 1), None, "inside edge untouched");
    assert_eq!(w(1, 1, 1), Some(2.0));
    assert_eq!(w(1, 2, 0), Some(2.0), "neighbour's facing side");
    assert_eq!(w(2, 1, 2), Some(2.0), "neighbour below");
    assert!(matches!(&t.rows[0].cells[0].borders[0].as_ref().unwrap().dash, Some(deckcraft_model::style::Dash::Preset { name }) if name == "dash"));
    // Inside horizontal only for the block.
    s.execute("table.borders", &json!({"id": id, "which": "insideH", "cells": block})).unwrap();
    let t = table(&s);
    assert!(t.rows[0].cells[0].borders[3].is_some() && t.rows[1].cells[0].borders[2].is_some());
    assert!(t.rows[0].cells[0].borders[1].is_none());
    // Diagonal and none.
    s.execute("table.borders", &json!({"id": id, "which": "diagDown", "cells": [[2, 2]]})).unwrap();
    assert!(table(&s).rows[2].cells[2].diag_down.is_some());
    s.execute("table.borders", &json!({"id": id, "which": "none"})).unwrap();
    let t = table(&s);
    assert!(
        t.rows
            .iter()
            .flat_map(|r| &r.cells)
            .all(|c| c.diag_down.is_none() && c.borders.iter().all(|b| b.as_ref().is_some_and(|l| l.fill == Some(deckcraft_model::Fill::None))))
    );
    assert!(s.execute("table.borders", &json!({"id": id, "which": "sideways"})).is_err());
    // Margins and vertical alignment.
    s.execute("table.cellMargins", &json!({"id": id, "preset": "wide", "cells": [[0, 0]]})).unwrap();
    s.execute("table.cellMargins", &json!({"id": id, "preset": "none", "left": 5.0, "cells": [[0, 1]]})).unwrap();
    s.execute("table.cellAlign", &json!({"id": id, "anchor": "middle"})).unwrap();
    let t = table(&s);
    assert_eq!(t.rows[0].cells[0].margins, Some([14.4; 4]));
    assert_eq!(t.rows[0].cells[1].margins, Some([5.0, 0.0, 0.0, 0.0]));
    assert!(t.rows.iter().flat_map(|r| &r.cells).all(|c| c.anchor == Some(deckcraft_model::text::Anchor::Middle)));
    // Survives PPTX.
    let p = s.doc().unwrap().doc.as_ref().clone();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    let bt = back
        .slides
        .iter()
        .flat_map(|sl| &sl.shapes)
        .find_map(|sh| if let deckcraft_model::ShapeKind::Table(t) = &sh.kind { Some(t.clone()) } else { None })
        .unwrap();
    assert_eq!(bt.rows[0].cells[0].margins, Some([14.4; 4]));
    assert_eq!(bt.rows[1].cells[1].anchor, Some(deckcraft_model::text::Anchor::Middle));
    // One undo step each.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(table(&s).rows[0].cells[0].anchor.is_none());
}

#[test]
fn smartart_text_pane_edits_regenerate_the_diagram() {
    let mut s = session();
    let r = s.execute("insert.smartArt", &json!({"kind": "blockList", "items": ["Plan", "\tScope", "Build"]})).unwrap();
    let gid = r["id"].as_u64().unwrap();
    let kids = |s: &Session| match s.doc().unwrap().shape(deckcraft_model::ShapeId(gid as u32)).map(|x| &x.kind) {
        Some(deckcraft_model::ShapeKind::Group { children, .. }) => children.clone(),
        _ => panic!("not a group"),
    };
    assert_eq!(kids(&s).len(), 2, "Scope is a bullet inside Plan");
    let box_before = s.doc().unwrap().shape(deckcraft_model::ShapeId(gid as u32)).unwrap().xfrm;

    // Type into the second shape directly, then add a node: the typed text is kept.
    let build = kids(&s)[1].id.0;
    s.execute("text.edit", &json!({"id": build, "end": true})).unwrap();
    s.execute("text.insert", &json!({"text": " it"})).unwrap();
    let r = s.execute("smartArt.addNode", &json!({"id": gid, "index": 2, "position": "after", "text": "Ship"})).unwrap();
    assert_eq!(r["index"], 3);
    let n = s.execute("smartArt.nodes", &json!({"id": gid})).unwrap();
    let texts: Vec<&str> = n["nodes"].as_array().unwrap().iter().map(|x| x["text"].as_str().unwrap()).collect();
    assert_eq!(texts, ["Plan", "Scope", "Build it", "Ship"]);
    assert_eq!(kids(&s).len(), 3);

    // Demote Ship under Build, promote Scope, remove Plan.
    s.execute("smartArt.demote", &json!({"id": gid, "index": 3})).unwrap();
    assert_eq!(kids(&s).len(), 2);
    s.execute("smartArt.promote", &json!({"id": gid, "index": 1})).unwrap();
    assert_eq!(kids(&s).len(), 3);
    assert!(s.execute("smartArt.promote", &json!({"id": gid, "index": 0})).is_err());
    s.execute("smartArt.removeNode", &json!({"id": gid, "index": 0})).unwrap();
    let n = s.execute("smartArt.nodes", &json!({"id": gid})).unwrap();
    let lv: Vec<u64> = n["nodes"].as_array().unwrap().iter().map(|x| x["level"].as_u64().unwrap()).collect();
    assert_eq!(lv, [0, 0, 1]);

    // A child shape also finds its diagram; layouts switch; the box stays put; undo works.
    let child = kids(&s)[0].id.0;
    s.execute("smartArt.layout", &json!({"id": child, "kind": "hierarchy"})).unwrap();
    assert_eq!(kids(&s).len(), 3 + 1, "three boxes and one link");
    assert_eq!(s.doc().unwrap().shape(deckcraft_model::ShapeId(gid as u32)).unwrap().xfrm, box_before);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(kids(&s).len(), 2);

    // The outline survives a .deckcraft save.
    let doc = s.doc().unwrap().doc.as_ref().clone();
    let bytes = deckcraft_format::save(&doc).unwrap();
    let back = deckcraft_format::load(&bytes).unwrap();
    assert!(back.slides[0].shapes.iter().any(|x| x.diagram.as_ref().is_some_and(|d| d.nodes.len() == 3)));
}

#[test]
fn smartart_round_trips_through_pptx_as_a_diagram() {
    let mut s = session();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.smartArt", &json!({"kind": "hierarchy", "items": ["CEO", "\tCTO", "\t\tDev", "\tCFO"]})).unwrap();
    s.execute("smartArt.style", &json!({"style": "intense"})).unwrap();
    s.execute("smartArt.colors", &json!({"colors": "accent3"})).unwrap();
    let doc = s.doc().unwrap().doc.as_ref().clone();
    let bytes = deckcraft_pptx::export(&doc).unwrap();
    let names: Vec<String> = {
        let z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        z.file_names().map(String::from).collect()
    };
    for part in ["data1", "layout1", "quickStyle1", "colors1", "drawing1"] {
        assert!(names.iter().any(|n| n == &format!("ppt/diagrams/{part}.xml")), "{part} in {names:?}");
    }
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert!(back.validate().is_empty());
    let g = back.slides[1].shapes.iter().find(|x| x.diagram.is_some()).expect("a SmartArt group");
    let d = g.diagram.as_ref().unwrap();
    assert_eq!(d.kind, "hierarchy");
    assert_eq!((d.style.as_str(), d.colors.as_str()), ("intense", "accent3"), "SmartArt style and colours come back");
    assert!(d.imported);
    let outline: Vec<(&str, u8)> = d.nodes.iter().map(|n| (n.text.as_str(), n.level)).collect();
    assert_eq!(outline, [("CEO", 0), ("CTO", 1), ("Dev", 2), ("CFO", 1)]);
    assert!(g.children().len() >= 4, "the cached drawing is read back as shapes");

    // The Text Pane works on the imported diagram, and a second round trip keeps the edit.
    let gid = g.id.0;
    let path = std::env::temp_dir().join(format!("deckcraft-smartart-{}.pptx", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let mut s2 = Session::new();
    s2.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    let _ = std::fs::remove_file(&path);
    s2.execute("slide.go", &json!({"index": 1})).unwrap();
    s2.execute("smartArt.addNode", &json!({"id": gid, "index": 3, "position": "after", "text": "COO"})).unwrap();
    let again = deckcraft_pptx::import(&deckcraft_pptx::export(s2.doc().unwrap().doc.as_ref()).unwrap()).unwrap();
    let d = again.slides[1].shapes.iter().find_map(|x| x.diagram.clone()).unwrap();
    assert_eq!(d.nodes.last().map(|n| (n.text.as_str(), n.level)), Some(("COO", 1)));
}

#[test]
fn smartart_pictures_3d_styles_and_colour_variants() {
    let mut s = session();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.smartArt", &json!({"kind": "pictureCaption", "items": ["Lakes", "\tBlue", "Hills"]})).unwrap();
    let pics = |s: &Session| -> Vec<deckcraft_model::Shape> {
        let st = s.doc().unwrap();
        let g = st.doc.slides[1].shapes.iter().find(|x| x.diagram.is_some()).unwrap().clone();
        g.children().iter().filter(|c| c.name == "SmartArt picture").cloned().collect()
    };
    assert_eq!(pics(&s).len(), 2, "one picture placeholder per top-level node");
    assert!(pics(&s).iter().all(|p| !matches!(p.fill, Some(deckcraft_model::Fill::Picture(_)))));
    // A 1×1 PNG put in through a sub-point lands in its top-level node.
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    s.execute("smartArt.picture", &json!({"index": 1, "name": "p.png", "data": png})).unwrap();
    let p = pics(&s);
    assert!(matches!(p[0].fill, Some(deckcraft_model::Fill::Picture(_))));
    assert!(!matches!(p[1].fill, Some(deckcraft_model::Fill::Picture(_))));
    // Text edits keep the picture; reset removes it.
    s.execute("smartArt.addNode", &json!({"text": "Woods"})).unwrap();
    assert!(matches!(pics(&s)[0].fill, Some(deckcraft_model::Fill::Picture(_))));
    assert_eq!(pics(&s).len(), 3);
    // The picture is written to the .pptx drawing.
    let bytes = deckcraft_pptx::export(s.doc().unwrap().doc.as_ref()).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    assert!(back.validate().is_empty());
    let bd = back.slides[1].shapes.iter().find_map(|x| x.diagram.as_ref()).unwrap();
    assert_eq!(bd.kind, "pictureCaption");
    // The node's picture comes back from the data model's blip, as its media.
    let m = bd.nodes[0].picture.expect("node picture read back");
    assert!(back.media(m).is_some_and(|x| !x.data.is_empty()));
    assert!(bd.nodes.iter().skip(1).all(|n| n.picture.is_none()));
    s.execute("smartArt.picture", &json!({"index": 0, "reset": true})).unwrap();
    assert!(pics(&s).iter().all(|p| !matches!(p.fill, Some(deckcraft_model::Fill::Picture(_)))));

    // 3-D styles carry a lit bevel; colour variants round trip through .pptx.
    s.execute("smartArt.style", &json!({"style": "cartoon"})).unwrap();
    let st = s.doc().unwrap();
    let g = st.doc.slides[1].shapes.iter().find(|x| x.diagram.is_some()).unwrap().clone();
    let raw = g.children().iter().find_map(|c| c.effects.as_ref().and_then(|e| e.raw3d.clone())).expect("3-D XML");
    assert!(raw.contains("scene3d") && raw.contains("softRound"));
    for (style, colors) in [("polished", "range34"), ("powder", "accent5Grad"), ("brick", "accent2")] {
        s.execute("smartArt.style", &json!({"style": style})).unwrap();
        s.execute("smartArt.colors", &json!({"colors": colors})).unwrap();
        let back = deckcraft_pptx::import(&deckcraft_pptx::export(s.doc().unwrap().doc.as_ref()).unwrap()).unwrap();
        let d = back.slides[1].shapes.iter().find_map(|x| x.diagram.clone()).unwrap();
        assert_eq!((d.style.as_str(), d.colors.as_str()), (style, colors));
    }
}

#[test]
fn smartart_converts_to_and_from_text_and_shapes() {
    let mut s = session();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.textBox", &json!({"text": "Goals\nRevenue\nGrowth"})).unwrap();
    let before = s.doc().unwrap().doc.slides[1].shapes.len();
    let gid = s.execute("smartArt.fromText", &json!({"kind": "blockList"})).unwrap()["id"].as_u64().unwrap();
    let st = s.doc().unwrap();
    assert_eq!(st.doc.slides[1].shapes.len(), before, "the text box is replaced, not added to");
    let d = st.doc.slides[1].shapes.iter().find_map(|x| x.diagram.clone()).unwrap();
    assert_eq!(d.nodes.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), ["Goals", "Revenue", "Growth"]);

    // Name and Title: titles show in their own box and survive a .pptx round trip.
    s.execute("smartArt.layout", &json!({"id": gid, "kind": "nameTitleOrgChart"})).unwrap();
    s.execute("smartArt.setNodes", &json!({"id": gid, "index": 0, "title": "Lead"})).unwrap();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(s.doc().unwrap().doc.as_ref()).unwrap()).unwrap();
    let bd = back.slides[1].shapes.iter().find_map(|x| x.diagram.clone()).unwrap();
    assert_eq!((bd.kind.as_str(), bd.nodes[0].title.as_str()), ("nameTitleOrgChart", "Lead"));

    let tid = s.execute("smartArt.toText", &json!({"id": gid})).unwrap()["id"].as_u64().unwrap();
    let st = s.doc().unwrap();
    let tb = st.doc.slides[1].shapes.iter().find(|x| x.id.0 as u64 == tid).unwrap();
    assert!(tb.diagram.is_none());
    assert_eq!(tb.text.as_ref().unwrap().paragraphs.len(), 3);
    // The text keeps the size it had in the graphic.
    assert!(tb.text.as_ref().unwrap().paragraphs.iter().all(|p| p.runs.iter().all(|r| r.props.size.is_some_and(|z| z > 10.0))));

    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("smartArt.toShapes", &json!({"id": gid})).unwrap();
    let st = s.doc().unwrap();
    let g = st.doc.slides[1].shapes.iter().find(|x| x.id.0 as u64 == gid).unwrap();
    assert!(g.diagram.is_none() && !g.children().is_empty(), "a plain group of the same shapes");
}

/// Action Settings: click and hover actions on the selected shape, removed again with `none`.
#[test]
fn action_settings_set_click_and_hover() {
    use deckcraft_model::text::Action;
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [0, 0, 50, 50]})).unwrap();
    let shape = |s: &Session| s.doc().unwrap().selected_shapes()[0].clone();
    s.execute("shape.action", &json!({"action": "slide", "slide": 0, "highlight": false})).unwrap();
    let first = s.doc().unwrap().doc.slides[0].id;
    let sh = shape(&s);
    assert!(matches!(sh.click.as_ref().map(|h| (&h.action, h.highlight_click)), Some((Action::Slide { slide }, false)) if *slide == first));
    s.execute("shape.action", &json!({"on": "hover", "action": "url", "url": "https://example.com"})).unwrap();
    assert!(matches!(shape(&s).hover.map(|h| h.action), Some(Action::Url { url }) if url == "https://example.com"));
    assert!(shape(&s).click.is_some(), "hover leaves the click action");
    s.execute("shape.action", &json!({"action": "none"})).unwrap();
    assert!(shape(&s).click.is_none());
    // Bad input is refused.
    assert!(s.execute("shape.action", &json!({"action": "slide", "slide": 99})).is_err());
    assert!(s.execute("shape.action", &json!({"action": "url"})).is_err());
    assert!(s.execute("shape.action", &json!({"action": "customShow", "show": "nope"})).is_err());
    assert!(s.execute("shape.action", &json!({"on": "drag", "action": "next"})).is_err());
}

#[test]
fn artistic_effects_apply_change_remove_and_round_trip() {
    use deckcraft_model::ShapeKind;
    let mut img = image::RgbaImage::new(40, 30);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = if x < 20 { image::Rgba([200, 30, 30, 255]) } else { image::Rgba([20, 20, 220, if y < 5 { 0 } else { 255 }]) };
    }
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("insert.picture", &json!({"name": "a.png", "data": cmd::base64_encode(&png)})).unwrap();
    let fill = |s: &Session| match &s.doc().unwrap().selected_shapes()[0].kind {
        ShapeKind::Picture { fill } => fill.clone(),
        k => panic!("{k:?}"),
    };
    let original = fill(&s).media;
    assert!(s.execute("picture.artisticEffect", &json!({"effect": "artisticBogus"})).is_err());
    // Every effect draws something of the same size and keeps transparency.
    for (e, _) in cmd::artistic::EFFECTS {
        s.execute("picture.artisticEffect", &json!({"effect": e})).unwrap();
        let f = fill(&s);
        assert_eq!(f.artistic.as_ref().map(|a| (a.effect.as_str(), a.original)), Some((*e, original)));
        assert_ne!(f.media, original);
        let data = s.doc().unwrap().doc.media(f.media).unwrap().data.clone();
        let out = image::load_from_memory(&data).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (40, 30));
        assert_eq!(out.get_pixel(30, 1)[3], 0, "{e}");
    }
    // A pencil sketch is grey.
    s.execute("picture.artisticEffect", &json!({"effect": "artisticPencilGrayscale"})).unwrap();
    let data = s.doc().unwrap().doc.media(fill(&s).media).unwrap().data.clone();
    let out = image::load_from_memory(&data).unwrap().to_rgba8();
    assert!(out.pixels().all(|p| p[0] == p[1] && p[1] == p[2]));
    // Saved and reopened, the effect and the original are both there.
    let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap();
    let mut t = Session::new();
    t.execute("file.openBytes", &json!({"name": "a.pptx", "data": cmd::base64_encode(&bytes)})).unwrap();
    let back = t.doc().unwrap().doc.slides.iter().flat_map(|sl| sl.shapes.iter()).find_map(|sh| match &sh.kind {
        ShapeKind::Picture { fill } => Some(fill.clone()),
        _ => None,
    });
    let back = back.unwrap();
    let art = back.artistic.clone().unwrap();
    assert_eq!(art.effect, "artisticPencilGrayscale");
    assert_eq!(*t.doc().unwrap().doc.media(art.original).unwrap().data, png);
    // None (and Reset Picture) puts the original back.
    s.execute("picture.artisticEffect", &json!({"effect": "none"})).unwrap();
    assert_eq!((fill(&s).media, fill(&s).artistic), (original, None));
    s.execute("picture.artisticEffect", &json!({"effect": "artisticBlur"})).unwrap();
    s.execute("picture.reset", &json!({})).unwrap();
    assert_eq!((fill(&s).media, fill(&s).artistic), (original, None));
}

#[test]
fn handout_master_view_edits_and_round_trips() {
    use deckcraft_model::PhType;
    let mut s = Session::with_new();
    assert!(s.doc().unwrap().doc.handout_master.is_none());
    assert!(s.execute("handout.placeholders", &json!({"footer": false})).is_err(), "only in the view");
    let r = s.execute("view.handoutMaster", &json!({})).unwrap();
    assert_eq!(r["created"], true);
    let st = s.doc().unwrap();
    assert_eq!(st.selection.target, Target::Handout);
    assert_eq!(crate::page_size(st), st.doc.notes_size);
    let kinds: Vec<_> = st.shapes().iter().filter_map(|x| x.ph_type()).collect();
    assert_eq!(kinds, [PhType::Header, PhType::Date, PhType::Footer, PhType::SlideNum]);
    // Hide the footer, then show it again; undo restores the hidden state.
    let r = s.execute("handout.placeholders", &json!({"footer": false})).unwrap();
    assert_eq!(r["footer"], false);
    assert_eq!(s.doc().unwrap().shapes().len(), 3);
    s.execute("handout.placeholders", &json!({"footer": true})).unwrap();
    assert_eq!(s.doc().unwrap().shapes().len(), 4);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().shapes().len(), 3);
    // Shapes drawn in the view land on the handout master, not the slide.
    let slide_shapes = s.doc().unwrap().doc.slides[0].shapes.len();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [20, 300, 100, 40]})).unwrap();
    assert_eq!(s.doc().unwrap().doc.handout_master.as_ref().unwrap().shapes.len(), 4);
    assert_eq!(s.doc().unwrap().doc.slides[0].shapes.len(), slide_shapes);
    assert!(crate::master_page(&s.doc().unwrap().doc, Target::Handout).is_some_and(|p| p.masters.len() == 1 && p.slide_size == p.notes_size));
    s.execute("view.closeMaster", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().selection.target, Target::Slides);
    // Reopening doesn't make a second master, and it survives PPTX.
    assert_eq!(s.execute("view.handoutMaster", &json!({})).unwrap()["created"], false);
    let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let hm = back.handout_master.expect("handout master kept");
    assert_eq!(hm.shapes.len(), 4);
    assert!(hm.shapes.iter().any(|x| x.ph_type() == Some(PhType::SlideNum)));
}

#[test]
fn notes_master_view_edits_and_saves() {
    use deckcraft_model::PhType;
    let mut s = Session::with_new();
    assert!(s.execute("notes.placeholders", &json!({"header": false})).is_err(), "only in the view");
    assert_eq!(s.execute("view.notesMaster", &json!({})).unwrap()["created"], true);
    let st = s.doc().unwrap();
    assert_eq!(st.selection.target, Target::NotesMaster);
    assert_eq!(crate::page_size(st), st.doc.notes_size);
    let kinds: Vec<_> = st.shapes().iter().filter_map(|x| x.ph_type()).collect();
    assert_eq!(kinds, [PhType::Header, PhType::Date, PhType::SlideImage, PhType::Body, PhType::Footer, PhType::SlideNum]);
    let r = s.execute("notes.placeholders", &json!({"header": false, "date": false})).unwrap();
    assert_eq!((r["header"].as_bool(), r["body"].as_bool()), (Some(false), Some(true)));
    assert_eq!(s.doc().unwrap().shapes().len(), 4);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().shapes().len(), 6);
    s.execute("notes.placeholders", &json!({"header": false})).unwrap();
    // A shape added in the view is on the notes master and is saved, even with no notes in the deck.
    s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [20, 20, 60, 60]})).unwrap();
    assert!(s.doc().unwrap().doc.slides.iter().all(|sl| sl.notes.is_empty()));
    assert!(crate::master_page(&s.doc().unwrap().doc, Target::NotesMaster).is_some());
    let bytes = deckcraft_pptx::export(&s.doc().unwrap().doc).unwrap();
    let back = deckcraft_pptx::import(&bytes).unwrap();
    let nm = back.notes_master.expect("notes master written");
    assert_eq!(nm.shapes.len(), 6, "{:?}", nm.shapes.iter().map(|x| &x.name).collect::<Vec<_>>());
    assert!(!nm.shapes.iter().any(|x| x.ph_type() == Some(PhType::Header)));
    s.execute("view.closeMaster", &json!({})).unwrap();
    assert_eq!(s.execute("view.notesMaster", &json!({})).unwrap()["created"], false);
}

/// Default picture export is 2 px per point. The file says 144 dpi, so Preview shows the slide's size.
#[test]
fn a_png_export_records_the_slide_resolution() {
    let mut s = session();
    let dir = std::env::temp_dir().join(format!("deckcraft-dpi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png = dir.join("slide.png");
    s.execute("file.export", &json!({"path": png.to_str().unwrap(), "format": "png"})).unwrap();
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(deckcraft_render::image_dpi(&bytes), Some((144.0, 144.0)));
    assert_eq!(png_size(&bytes), Some((1920, 1080)), "960×540 pt at 2 px per point");
    let one = dir.join("one.png");
    s.execute("file.export", &json!({"path": one.to_str().unwrap(), "format": "png", "scale": 1})).unwrap();
    let one_bytes = std::fs::read(&one).unwrap();
    assert_eq!(deckcraft_render::image_dpi(&one_bytes), Some((72.0, 72.0)));
    assert_eq!(png_size(&one_bytes), Some((960, 540)));
    let jpg = dir.join("slide.jpg");
    s.execute("file.export", &json!({"path": jpg.to_str().unwrap(), "format": "jpeg"})).unwrap();
    assert_eq!(deckcraft_render::image_dpi(&std::fs::read(&jpg).unwrap()), Some((144.0, 144.0)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// #784: `transparentBackground` leaves the slide background out of a PNG export, so only the
/// slide's content is opaque; formats without transparency refuse it and write nothing.
#[test]
fn a_png_export_can_leave_the_background_transparent() {
    let mut s = session();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    s.execute("design.background", &json!({"color": "#0000C8"})).unwrap();
    let dir = std::env::temp_dir().join(format!("deckcraft-transparent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let read = |path: &std::path::Path| image::load_from_memory(&std::fs::read(path).unwrap()).unwrap().to_rgba8();
    let opaque = dir.join("opaque.png");
    s.execute("file.export", &json!({"path": opaque.to_str().unwrap(), "scale": 1})).unwrap();
    assert_eq!(read(&opaque).get_pixel(5, 5).0, [0, 0, 200, 255], "the background draws by default");
    let clear = dir.join("clear.png");
    s.execute("file.export", &json!({"path": clear.to_str().unwrap(), "scale": 1, "transparentBackground": true})).unwrap();
    let img = read(&clear);
    assert_eq!(img.get_pixel(5, 5).0[3], 0, "no background: {:?}", img.get_pixel(5, 5));
    assert_eq!(img.get_pixel(200, 150).0[3], 255, "the shape stays opaque: {:?}", img.get_pixel(200, 150));
    for (name, format) in [("slide.jpg", "jpeg"), ("deck.pdf", "pdf"), ("deck.gif", "gif"), ("deck.pptx", "pptx")] {
        let path = dir.join(name);
        let e = s.execute("file.export", &json!({"path": path.to_str().unwrap(), "format": format, "transparentBackground": true})).unwrap_err();
        assert!(e.to_string().contains("for PNG export"), "{format}: {e}");
        assert!(!path.exists(), "{format}: nothing is written");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let w: [u8; 4] = bytes.get(16..20)?.try_into().ok()?;
    let h: [u8; 4] = bytes.get(20..24)?.try_into().ok()?;
    Some((u32::from_be_bytes(w), u32::from_be_bytes(h)))
}

/// A blank slide holding one text box being edited, with smart guides and grid snapping off so
/// drags move by exactly the pointer delta. Returns the session, the box id and its box.
fn editing_text_box(rot: f64) -> (Session, u64, deckcraft_geom::Xfrm) {
    let mut s = session();
    s.prefs.snap_to_grid = false;
    s.prefs.smart_guides = false;
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = s.execute("insert.textBox", &json!({"rect": [200.0, 150.0, 240.0, 60.0], "text": "Hello world"})).unwrap()["id"].as_u64().unwrap();
    if rot != 0.0 {
        s.execute("shape.rotate", &json!({"ids": [id], "deg": rot})).unwrap();
    }
    s.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
    let x = text_box_xfrm(&s, id);
    (s, id, x)
}

/// Slide point for a point in the box's own (rotated) frame.
fn on_box(x: &deckcraft_geom::Xfrm, lx: f64, ly: f64) -> (f64, f64) {
    let p = x.affine() * deckcraft_geom::Point::new(lx, ly);
    (p.x, p.y)
}

fn drag_gesture(s: &mut Session, from: (f64, f64), to: (f64, f64)) -> serde_json::Value {
    let ev = |kind, (x, y): (f64, f64)| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    let down = s.pointer(ev(PointerKind::Down, from)).unwrap();
    let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    s.pointer(ev(PointerKind::Drag, mid)).unwrap();
    s.pointer(ev(PointerKind::Drag, to)).unwrap();
    s.pointer(ev(PointerKind::Up, to)).unwrap();
    down
}

#[test]
fn dragging_the_frame_of_edited_text_moves_the_box() {
    let (mut s, id, x) = editing_text_box(0.0);
    assert!(x.h > 12.0, "{x:?}");
    let sid = deckcraft_model::ShapeId(id as u32);
    // Hovering the band on either side of the edge says "move"; the inside says "text".
    for ly in [-2.0, 2.0, x.h + 2.0] {
        let (px, py) = on_box(&x, x.w / 4.0, ly);
        s.pointer(PointerEvent { kind: PointerKind::Move, x: px, y: py, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap();
        assert_eq!(s.tool.hover, Some(tools::Hit::Frame { id: sid }), "ly {ly}");
    }
    let (px, py) = on_box(&x, x.w / 4.0, x.h / 2.0);
    s.pointer(PointerEvent { kind: PointerKind::Move, x: px, y: py, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap();
    assert_eq!(s.tool.hover, Some(tools::Hit::Shape { id: sid, text: true }));

    // Press on the top edge, just outside, and drag: the box moves, text editing ends.
    let undo_before = s.doc().unwrap().history.undo.len();
    let from = on_box(&x, x.w / 4.0, -1.0);
    let down = drag_gesture(&mut s, from, (from.0 + 30.0, from.1 + 20.0));
    assert_eq!(down["hit"], json!({"kind": "frame", "id": id}));
    let moved = text_box_xfrm(&s, id);
    assert!((moved.x - (x.x + 30.0)).abs() < 1e-9 && (moved.y - (x.y + 20.0)).abs() < 1e-9, "{moved:?}");
    assert_eq!((moved.w, moved.h), (x.w, x.h));
    let st = s.doc().unwrap();
    assert!(st.selection.text.is_none());
    assert_eq!(st.selection.shapes, vec![sid]);
    // One undo step, which puts the box back.
    assert_eq!(st.history.undo.len(), undo_before + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text_box_xfrm(&s, id), x);

    // The inner side of the band works too; a click there without a drag selects the box whole
    // and changes nothing.
    s.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
    let undo_before = s.doc().unwrap().history.undo.len();
    let (px, py) = on_box(&x, x.w * 0.75, x.h - 1.0);
    let ev = |kind| PointerEvent { kind, x: px, y: py, mods: Mods::default(), tol: 3.0, pressure: None };
    s.pointer(ev(PointerKind::Down)).unwrap();
    s.pointer(ev(PointerKind::Up)).unwrap();
    let st = s.doc().unwrap();
    assert!(st.selection.text.is_none());
    assert_eq!(st.selection.shapes, vec![sid]);
    assert_eq!(st.history.undo.len(), undo_before);
    assert_eq!(text_box_xfrm(&s, id), x);
}

#[test]
fn dragging_inside_edited_text_selects_text() {
    let (mut s, id, x) = editing_text_box(0.0);
    let undo_before = s.doc().unwrap().history.undo.len();
    let down = drag_gesture(&mut s, on_box(&x, 8.0, x.h / 2.0), on_box(&x, x.w - 8.0, x.h / 2.0));
    assert!(down.get("caret").is_some(), "{down}");
    assert_eq!(text_box_xfrm(&s, id), x);
    let st = s.doc().unwrap();
    let t = st.selection.text.as_ref().expect("still editing");
    assert!(t.is_range(), "{t:?}");
    assert_eq!(t.ordered(), ((0, 0), (0, 11)));
    assert_eq!(st.history.undo.len(), undo_before);
}

#[test]
fn frame_of_rotated_edited_text_follows_the_rotation() {
    let (mut s, id, x) = editing_text_box(30.0);
    assert_eq!(x.rot, 30.0);
    let sid = deckcraft_model::ShapeId(id as u32);
    // The rotated bottom edge is the frame; dragging it moves the box and keeps its rotation.
    let from = on_box(&x, x.w / 4.0, x.h + 1.0);
    assert_eq!(s.hit_test(deckcraft_geom::Point::new(from.0, from.1), 3.0), Some(tools::Hit::Frame { id: sid }));
    let down = drag_gesture(&mut s, from, (from.0 + 25.0, from.1 - 10.0));
    assert_eq!(down["hit"]["kind"], "frame");
    let moved = text_box_xfrm(&s, id);
    assert!((moved.x - (x.x + 25.0)).abs() < 1e-9 && (moved.y - (x.y - 10.0)).abs() < 1e-9, "{moved:?}");
    assert_eq!(moved.rot, 30.0);
    // Inside the rotated box, pressing still places the caret.
    s.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
    let mx = text_box_xfrm(&s, id);
    let (px, py) = on_box(&mx, mx.w / 2.0, mx.h / 2.0);
    let r = s.pointer(PointerEvent { kind: PointerKind::Down, x: px, y: py, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap();
    s.pointer(PointerEvent { kind: PointerKind::Up, x: px, y: py, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap();
    assert!(r.get("caret").is_some(), "{r}");
    assert_eq!(text_box_xfrm(&s, id), mx);
    // Inside the axis-aligned bounds but off the rotated box: neither text nor frame, so a press
    // there leaves editing instead of moving the caret.
    let off = on_box(&mx, mx.w / 2.0, -mx.h);
    let off_pt = deckcraft_geom::Point::new(off.0, off.1);
    assert!(mx.bounds().contains(off_pt));
    assert_ne!(s.hit_test(off_pt, 3.0), Some(tools::Hit::Frame { id: sid }));
    let r = s.pointer(PointerEvent { kind: PointerKind::Down, x: off.0, y: off.1, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap();
    s.pointer(PointerEvent { kind: PointerKind::Up, x: off.0, y: off.1, mods: Mods::default(), tol: 3.0, pressure: None }).unwrap();
    assert!(r.get("caret").is_none(), "{r}");
    assert!(s.doc().unwrap().selection.text.is_none());
    assert_eq!(text_box_xfrm(&s, id), mx);
}

#[test]
fn resize_handles_beat_the_frame_of_edited_text() {
    let (mut s, id, x) = editing_text_box(0.0);
    // The SE corner lies in the frame band, but its handle wins.
    let from = on_box(&x, x.w, x.h);
    let down = drag_gesture(&mut s, from, (from.0 + 40.0, from.1));
    assert_eq!(down["hit"]["kind"], "resize");
    let sized = text_box_xfrm(&s, id);
    assert!((sized.w - (x.w + 40.0)).abs() < 1e-9, "{sized:?}");
    assert_eq!((sized.x, sized.y), (x.x, x.y));
}

#[test]
fn format_language_marks_the_selected_text() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Hola mundo"})).unwrap();
    s.execute("text.move", &json!({"to": "wordLeft", "extend": true})).unwrap();
    s.execute("format.language", &json!({"lang": "es-ES"})).unwrap();
    assert_eq!(s.execute("format.state", &json!({})).unwrap()["lang"], "es-ES");
    let body = s.doc().unwrap().current_slide().unwrap().shapes[0].text.clone().unwrap();
    let langs: Vec<_> = body.paragraphs[0].runs.iter().map(|r| (r.text.as_str(), r.props.lang.as_deref())).collect();
    assert_eq!(langs, vec![("Hola ", None), ("mundo", Some("es-ES"))]);
}

#[test]
fn every_params_doc_names_its_keys() {
    for c in command_specs() {
        assert!(cmd::param_keys(c.params).is_ok(), "{}: {:?}", c.id, cmd::param_keys(c.params));
    }
    assert_eq!(cmd::param_keys("{index} | {id}").unwrap(), ["index", "id"]);
    assert_eq!(cmd::param_keys("{color? | none?: true, kind?: a|b (x, y), ids?} → {id}").unwrap(), ["color", "none", "kind", "ids"]);
}

#[test]
fn a_params_doc_spells_out_each_parameter() {
    use cmd::Param;
    let p = |name, optional, about, or, object| Param { name, optional, about, or, object };
    assert_eq!(
        cmd::parse_params("{preset: rect|roundRect, rect?: [x,y,w,h] pt, index? (else selected shapes)} → {id}").unwrap(),
        [
            p("preset", false, "rect|roundRect", false, 0),
            p("rect", true, "[x,y,w,h] pt", false, 0),
            p("index", true, "(else selected shapes)", false, 0)
        ]
    );
    assert_eq!(
        cmd::parse_params("{color? | none?: true, gradient?: {stops, kind?: a|b}, ids?}").unwrap(),
        [
            p("color", true, "", false, 0),
            p("none", true, "true", true, 0),
            p("gradient", true, "{stops, kind?: a|b}", false, 0),
            p("ids", true, "", false, 0)
        ]
    );
    assert_eq!(
        cmd::parse_params("{name} | {major, minor}").unwrap(),
        [p("name", false, "", false, 0), p("major", false, "", false, 1), p("minor", false, "", false, 1)]
    );
}

#[test]
fn what_a_command_acts_on_names_only_keys_it_takes() {
    for c in command_specs() {
        let keys = cmd::param_keys(c.params).unwrap();
        let said = c.acts_on().unwrap_or_default();
        assert!(!said.contains("`ids`") || keys.contains(&"ids"), "{}: {said}", c.id);
        assert!(!said.contains("`id`") || keys.contains(&"id"), "{}: {said}", c.id);
    }
    let acts = |id: &str| find_command(id).and_then(|c| c.acts_on());
    assert_eq!(acts("shape.resize").as_deref(), Some("Acts on the shapes in `ids`, else on the selected shapes."));
    assert_eq!(acts("table.style").as_deref(), Some("Acts on the shape in `id`, else on the selected shapes."));
    assert_eq!(acts("shape.setDefault").as_deref(), Some("Acts on the selected shapes."));
    assert_eq!(acts("format.size").as_deref(), Some("Acts on the selected text or shapes."));
    assert_eq!(acts("slide.new"), None);
}

/// Tables whose style the file defines (#292, #294: its look and background drawn) edit like the
/// others: the caret is laid out with the file style's lettering, clicks and double-clicks land
/// in the cell where it is drawn, and borders still resize.
#[test]
fn tables_with_a_file_defined_style_edit_and_resize() {
    use deckcraft_model::style::{ColorRef, Fill};
    use deckcraft_model::table::{TablePart, TablePartStyle, TableStyleDef};
    let mut s = session();
    let id = table_at(&mut s, &[200.0, 200.0], json!([["Hello world", "b"], ["c", "d"]]));
    s.execute("text.edit", &json!({"id": id, "cell": [0, 0]})).unwrap();
    let built_in = caret_point(&s, 0, 8);
    s.execute("text.exit", &json!({})).unwrap();
    // The table's style, defined by the file: a plain (not bold) header, a filled body, a
    // background.
    {
        let style = table_of(&s, id).style;
        let d = s.doc_mut().unwrap();
        let doc = std::sync::Arc::make_mut(&mut d.doc);
        let solid = |r, g, b| Some(Fill::solid(ColorRef::rgb(deckcraft_color::Rgba::rgb(r, g, b))));
        doc.table_styles.push(TableStyleDef {
            id: style,
            name: "File style".into(),
            parts: vec![
                (TablePart::WholeTbl, TablePartStyle { fill: solid(230, 240, 250), ..Default::default() }),
                (TablePart::FirstRow, TablePartStyle { fill: solid(40, 60, 90), bold: Some(false), ..Default::default() }),
            ],
            background: Some(TablePartStyle { fill: solid(200, 200, 200), ..Default::default() }),
            raw: String::new(),
        });
    }
    s.execute("edit.deselect", &json!({})).unwrap();
    s.execute("text.edit", &json!({"id": id, "cell": [0, 0]})).unwrap();
    let p = caret_point(&s, 0, 8);
    assert!((p.x - built_in.x).abs() > 0.5, "the header is laid out in the file style's (regular) weight: {p:?} vs {built_in:?}");
    s.execute("text.exit", &json!({})).unwrap();
    s.execute("edit.deselect", &json!({})).unwrap();
    // A double-click selects the word drawn there.
    s.pointer(tev(PointerKind::DoubleClick, p.x, p.y)).unwrap();
    let t = text_sel(&s).unwrap();
    assert_eq!((t.cell, t.ordered()), (Some((0, 0)), ((0, 6), (0, 11))));
    // A click in another cell moves the caret there.
    s.pointer(tev(PointerKind::Down, 400.0, 160.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 400.0, 160.0)).unwrap();
    assert_eq!(text_sel(&s).unwrap().cell, Some((1, 1)));
    // Border drag, while editing: the columns resize, editing goes on.
    s.pointer(tev(PointerKind::Down, 300.0, 120.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 320.0, 120.0)).unwrap();
    s.pointer(tev(PointerKind::Drag, 330.0, 121.0)).unwrap();
    s.pointer(tev(PointerKind::Up, 330.0, 121.0)).unwrap();
    assert_eq!(table_of(&s, id).cols, vec![230.0, 170.0]);
    assert_eq!(text_sel(&s).unwrap().cell, Some((1, 1)));
}

#[test]
fn editing_notes_keeps_their_formatting_through_pptx() {
    use deckcraft_model::text::{Bullet, ParaProps, Paragraph, Run, RunProps, TextBody};
    // A deck whose notes have a bold heading over indented bullet points, as saved by PowerPoint.
    let mut p = deckcraft_model::defaults::new_presentation(None);
    let bullet = ParaProps { margin_left: Some(22.5), indent: Some(-22.5), bullet: Some(Bullet::Char { char: "•".into() }), ..Default::default() };
    let point = |t: &str| Paragraph { level: 1, props: bullet.clone(), runs: vec![Run::new(t)], ..Default::default() };
    std::sync::Arc::make_mut(&mut p.slides[0]).notes = TextBody {
        paragraphs: vec![
            Paragraph { runs: vec![Run::with("Overview", RunProps { bold: Some(true), ..Default::default() })], ..Default::default() },
            point("First point"),
            point("Second point"),
        ],
        ..Default::default()
    };
    let bytes = deckcraft_pptx::export(&p).unwrap();
    let mut s = session();
    s.execute("file.openBytes", &json!({"name": "notes.pptx", "data": crate::cmd::base64_encode(&bytes)})).unwrap();
    // Type a word into the second bullet, as the notes pane does.
    s.execute("slide.notes", &json!({"index": 0, "text": "Overview\nFirst point\nSecond main point"})).unwrap();
    let check = |notes: &TextBody| {
        assert_eq!(notes.text(), "Overview\nFirst point\nSecond main point");
        assert_eq!(notes.paragraphs[0].runs[0].props.bold, Some(true));
        for para in &notes.paragraphs[1..] {
            assert_eq!(para.level, 1);
            assert_eq!(para.props.margin_left, Some(22.5));
            assert_eq!(para.props.indent, Some(-22.5));
            assert_eq!(para.props.bullet, Some(Bullet::Char { char: "•".into() }));
        }
    };
    check(&s.doc().unwrap().doc.slides[0].notes);
    let saved = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].as_str().unwrap().to_string();
    let reopened = deckcraft_pptx::import(&crate::cmd::base64_decode(&saved).unwrap()).unwrap();
    check(&reopened.slides[0].notes);
    // slide.inspect hands out the formatted notes, and slide.notes {body} takes them back as they are.
    let body = s.execute("slide.inspect", &json!({"index": 0})).unwrap()["notesBody"].clone();
    s.execute("slide.notes", &json!({"index": 0, "text": "Plain"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides[0].notes, TextBody::from_text("Plain"), "new text shares nothing: plain");
    s.execute("slide.notes", &json!({"index": 0, "body": body})).unwrap();
    check(&s.doc().unwrap().doc.slides[0].notes);
    assert!(s.execute("slide.notes", &json!({"index": 0, "body": 7})).is_err());
}

#[test]
fn autocorrect_options_as_you_type() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    let typed = |s: &mut Session, text: &str| -> String {
        s.execute("text.edit", &json!({"id": title})).unwrap();
        s.execute("text.selectAll", &json!({})).ok();
        s.execute("text.delete", &json!({"dir": "backward"})).ok();
        for c in text.chars() {
            s.execute("text.insert", &json!({"text": c.to_string()})).unwrap();
        }
        s.execute("text.get", &json!({"id": title})).unwrap()["text"].as_str().unwrap().to_string()
    };
    assert_eq!(typed(&mut s, "hello world. see you monday, i think. "), "Hello world. See you Monday, I think. ");
    assert_eq!(typed(&mut s, "THe tHIS 1/2 recieved... "), "The This ½ received… ");
    // Options turn rules off.
    s.execute("text.autocorrect", &json!({"sentenceCaps": false, "days": false})).unwrap();
    assert_eq!(typed(&mut s, "hello monday "), "hello monday ");
    // Your own entries, and deleting a built-in one.
    s.execute("text.autocorrect", &json!({"add": ["dc", "DeckCraft"], "remove": "teh"})).unwrap();
    assert_eq!(typed(&mut s, "dc teh "), "DeckCraft teh ");
    let v = s.execute("text.autocorrect", &json!({})).unwrap();
    let entries = v["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e[0] == "dc"));
    assert!(!entries.iter().any(|e| e[0] == "teh"));
    assert!(s.execute("text.autocorrect", &json!({"add": ["two words", "x"]})).is_err());
    // Off entirely.
    s.execute("text.autocorrect", &json!({"on": false})).unwrap();
    assert_eq!(typed(&mut s, "dc adn "), "dc adn ");
    // Pasting a whole string applies only the replacement list, not the case rules.
    s.execute("text.autocorrect", &json!({"on": true, "sentenceCaps": true})).unwrap();
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.selectAll", &json!({})).ok();
    s.execute("text.insert", &json!({"text": "monday notes "})).unwrap();
    assert_eq!(s.execute("text.get", &json!({"id": title})).unwrap()["text"], "monday notes ");
}

#[test]
fn autocorrect_superscripts_ordinals_as_you_type() {
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    // The typed text with each superscript run in ^(…).
    let typed = |s: &mut Session, text: &str| -> String {
        s.execute("text.edit", &json!({"id": title})).unwrap();
        s.execute("text.selectAll", &json!({})).ok();
        s.execute("text.delete", &json!({"dir": "backward"})).ok();
        for c in text.chars() {
            s.execute("text.insert", &json!({"text": c.to_string()})).unwrap();
        }
        let st = s.doc().unwrap();
        let sh = st.current_slide().unwrap().shapes.iter().find(|sh| sh.id.0 == title).unwrap();
        sh.text.as_ref().unwrap().paragraphs[0]
            .runs
            .iter()
            .map(|r| if r.props.baseline.unwrap_or(0.0) > 0.0 { format!("^({})", r.text) } else { r.text.clone() })
            .collect()
    };
    assert_eq!(typed(&mut s, "1st, 22nd and 103rd. 4th 11th x"), "1^(st), 22^(nd) and 103^(rd). 4^(th) 11^(th) x");
    // Not a word, a number without a suffix it takes, or a suffix not yet followed by a space.
    assert_eq!(typed(&mut s, "first x1st 1th 2st 21st"), "First x1st 1th 2st 21st");
    assert_eq!(s.execute("text.autocorrect", &json!({})).unwrap()["ordinals"], true);
    s.execute("text.autocorrect", &json!({"ordinals": false})).unwrap();
    assert_eq!(typed(&mut s, "2nd "), "2nd ");
    // AutoCorrect off turns it off too.
    s.execute("text.autocorrect", &json!({"ordinals": true, "on": false})).unwrap();
    assert_eq!(typed(&mut s, "2nd "), "2nd ");
}

#[test]
fn theme_variants_apply_and_report_the_current_one() {
    let mut s = session();
    let name = deckcraft_model::theme::builtin_themes()[1].name.clone();
    s.execute("design.theme", &json!({"name": name})).unwrap();
    let v = s.execute("design.variants", &json!({})).unwrap();
    assert_eq!(v["variants"].as_array().unwrap().len(), 4);
    assert_eq!(v["current"], json!(0));
    let base = s.doc().unwrap().doc.masters[0].theme.colors.clone();
    s.execute("design.variant", &json!({"index": 1})).unwrap();
    let c = s.doc().unwrap().doc.masters[0].theme.colors.clone();
    assert_eq!(c.colors[4], base.colors[5], "accents rotate");
    assert_eq!(c.colors[0], base.colors[0]);
    assert_eq!(s.execute("design.variants", &json!({})).unwrap()["current"], json!(1));
    // From a variant, another variant is built from the theme's own colours, not the variant's.
    s.execute("design.variant", &json!({"index": 3})).unwrap();
    let d = s.doc().unwrap().doc.masters[0].theme.colors.clone();
    assert_eq!((d.colors[0], d.colors[1], d.colors[4]), (base.colors[1], base.colors[0], base.colors[4]));
    s.execute("design.variant", &json!({"index": 0})).unwrap();
    assert_eq!(s.doc().unwrap().doc.masters[0].theme.colors.colors, base.colors);
    assert!(s.execute("design.variant", &json!({"index": 9})).is_err());
    // It survives PPTX.
    s.execute("design.variant", &json!({"index": 2})).unwrap();
    let p = s.doc().unwrap().doc.as_ref().clone();
    let back = deckcraft_pptx::import(&deckcraft_pptx::export(&p).unwrap()).unwrap();
    assert_eq!(back.masters[0].theme.colors.colors, p.masters[0].theme.colors.colors);
}

#[test]
fn spelling_uses_a_dictionary_suggests_and_changes() {
    crate::spell::install("xx-TEST", crate::spell::Dictionary::parse(crate::spell::tests::AFF.as_bytes(), crate::spell::tests::DIC.as_bytes()));
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 300, 100], "text": "Making sldie photos, sldie"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    let v = s.execute("review.spelling", &json!({"lang": "xx-TEST"})).unwrap();
    let list = v.as_array().unwrap();
    assert_eq!(list.len(), 2, "{v}");
    assert_eq!(list[0]["word"], "sldie");
    assert_eq!(list[0]["suggestions"][0], "slide");
    assert!(s.execute("review.spelling", &json!({"lang": "xx-TEST", "ignore": ["SLDIE"]})).unwrap().as_array().unwrap().is_empty());
    let c = s.execute("review.spellingChange", &json!({"shape": id, "word": "sldie", "to": "slide"})).unwrap();
    assert_eq!(c["changed"], 1);
    assert_eq!(s.execute("review.spelling", &json!({"lang": "xx-TEST"})).unwrap().as_array().unwrap().len(), 1);
    let c = s.execute("review.spellingChange", &json!({"word": "sldie", "to": "slide", "all": true})).unwrap();
    assert_eq!(c["changed"], 1);
    assert!(s.execute("review.spelling", &json!({"lang": "xx-TEST"})).unwrap().as_array().unwrap().is_empty());
    // Whole words only, and one undo step per change.
    assert_eq!(s.execute("review.spellingChange", &json!({"word": "slid", "to": "x", "all": true})).unwrap()["changed"], 0);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("review.spelling", &json!({"lang": "xx-TEST"})).unwrap().as_array().unwrap().len(), 1);
}

#[test]
fn spelling_without_a_dictionary_falls_back_to_common_misspellings() {
    let mut s = session();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 300, 100], "text": "Teh qzxv"})).unwrap();
    let v = s.execute("review.spelling", &json!({"lang": "zz-NONE"})).unwrap();
    assert_eq!(v.as_array().map(Vec::len), Some(1));
    assert_eq!(v[0]["word"], "Teh");
    let d = s.execute("review.dictionary", &json!({"lang": "zz-NONE"})).unwrap();
    assert!(d["found"].is_null());
    assert_eq!(d["available"], false);
}

#[test]
fn spelling_checks_each_run_in_its_language_and_learns_words() {
    let dir = std::env::temp_dir().join(format!("deckcraft-userdic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::spell::set_user_dictionary_path(dir.join("user.dic"));
    crate::spell::install("xx-ONE", crate::spell::Dictionary::parse(b"", b"2\nhello\nworld\n"));
    crate::spell::install("xx-TWO", crate::spell::Dictionary::parse(b"", b"2\nhallo\nwelt\n"));
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 300, 100], "text": "hello hallo deckcraft"})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    // The second word is in another language (as a PPTX run's `lang` gives it).
    s.edit(|doc, _| {
        let sh = deckcraft_model::find_shape_mut(&mut std::sync::Arc::make_mut(&mut doc.slides[0]).shapes, id).unwrap();
        let p = &mut sh.text.as_mut().unwrap().paragraphs[0];
        let t = p.runs[0].text.clone();
        let props = p.runs[0].props.clone();
        p.runs = vec![
            deckcraft_model::Run::with(&t[..6], deckcraft_model::RunProps { lang: Some("xx-ONE".into()), ..props.clone() }),
            deckcraft_model::Run::with(&t[6..12], deckcraft_model::RunProps { lang: Some("xx-TWO".into()), ..props.clone() }),
            deckcraft_model::Run::with(&t[12..], deckcraft_model::RunProps { lang: Some("xx-ONE".into()), ..props }),
        ];
        Ok(())
    })
    .unwrap();
    let v = s.execute("review.spelling", &json!({})).unwrap();
    let words: Vec<_> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|w| (w["word"].as_str().unwrap().to_string(), w["lang"].as_str().unwrap().to_string(), w["start"].as_u64().unwrap()))
        .collect();
    assert_eq!(words, vec![("deckcraft".to_string(), "xx-ONE".to_string(), 12)]);
    s.execute("review.addToDictionary", &json!({"word": "deckcraft"})).unwrap();
    assert!(s.execute("review.spelling", &json!({})).unwrap().as_array().unwrap().is_empty());
    assert_eq!(std::fs::read_to_string(dir.join("user.dic")).unwrap(), "deckcraft\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn language_marks_text_and_no_proof_and_repeats_are_reported() {
    crate::spell::install("xx-LN", crate::spell::Dictionary::parse(b"", b"3\nthe\nend\nis\n"));
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 300, 100], "text": "the the end is nigh"})).unwrap();
    let id = r["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.execute("format.language", &json!({"lang": "xx-LN"})).unwrap();
    let v = s.execute("review.spelling", &json!({})).unwrap();
    let got: Vec<(String, bool)> = v.as_array().unwrap().iter().map(|w| (w["word"].as_str().unwrap().to_string(), w["repeated"] == true)).collect();
    assert_eq!(got, vec![("the".to_string(), true), ("nigh".to_string(), false)]);
    assert_eq!(v[0]["deleteFrom"], 3);
    // Deleting the repeat through the exact-occurrence change.
    s.execute("review.spellingChange", &json!({"shape": id, "para": 0, "start": 3, "word": " the", "to": ""})).unwrap();
    let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
    assert_eq!(v["shape"]["text"]["paragraphs"][0]["runs"][0]["text"], "the end is nigh");
    s.execute("format.language", &json!({"noProof": true})).unwrap();
    assert!(s.execute("review.spelling", &json!({})).unwrap().as_array().unwrap().is_empty());
    assert!(s.execute("format.language", &json!({"lang": "en US!"})).is_err());
}

#[test]
fn the_caret_takes_the_colour_of_the_text() {
    let caret = |theme: &str| {
        let mut s = session();
        s.execute("design.theme", &json!({"name": theme})).unwrap();
        let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
        s.execute("text.edit", &json!({"id": title})).unwrap();
        let st = s.doc().unwrap();
        let empty = cmd::text::caret_color(st, &st.selection.text.clone().unwrap()).unwrap();
        s.execute("text.insert", &json!({"text": "Typed"})).unwrap();
        let st = s.doc().unwrap();
        assert_eq!(cmd::text::caret_color(st, &st.selection.text.clone().unwrap()), Some(empty));
        empty
    };
    // Dark text on the light theme, light text on the dark one: never black on a dark slide.
    let (light, dark) = (caret("DeckCraft"), caret("Nocturne"));
    assert!(light.r < 0x60 && light.g < 0x60 && light.b < 0x60, "{light:?}");
    assert!(dark.r > 0xC0 && dark.g > 0xC0 && dark.b > 0xC0, "{dark:?}");
    // Text set to red gets a red caret.
    let mut s = session();
    let title = s.doc().unwrap().current_slide().unwrap().shapes[0].id.0;
    s.execute("text.edit", &json!({"id": title})).unwrap();
    s.execute("text.insert", &json!({"text": "Red"})).unwrap();
    s.execute("text.selectAll", &json!({})).unwrap();
    s.execute("format.color", &json!({"color": "#FF0000"})).unwrap();
    s.execute("text.move", &json!({"to": "end"})).unwrap();
    let st = s.doc().unwrap();
    let red = cmd::text::caret_color(st, &st.selection.text.clone().unwrap()).unwrap();
    assert_eq!((red.r, red.g, red.b), (0xFF, 0, 0));
}

/// Smooth Corners (#286): a rounded rectangle becomes a squircle outline that survives PPTX.
#[test]
fn smooth_corners_makes_a_squircle_outline() {
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "roundRect", "rect": [100, 100, 200, 100]})).unwrap();
    let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
    s.execute("shape.smoothCorners", &json!({"smoothing": 60})).unwrap();
    let d = |s: &Session| match &s.doc().unwrap().shape(id).unwrap().geom {
        deckcraft_model::Geom::Custom { paths } => paths[0].d.clone(),
        g => panic!("{g:?}"),
    };
    // The default roundRect radius is 16.667% of the short side: 16.667 pt, smoothed 60%.
    assert_eq!(d(&s), deckcraft_geom::squircle::path_d(200.0, 100.0, 16.667, 0.6));
    // The smoothed outline takes a new smoothing with the radius it was drawn with; without a
    // radius, or with another one (it could be any freeform), it is refused.
    s.execute("shape.smoothCorners", &json!({"smoothing": 100, "radius": 16.667})).unwrap();
    assert_eq!(d(&s), deckcraft_geom::squircle::path_d(200.0, 100.0, 16.667, 1.0));
    assert!(s.execute("shape.smoothCorners", &json!({"smoothing": 50})).is_err());
    assert!(s.execute("shape.smoothCorners", &json!({"smoothing": 50, "radius": 30})).is_err());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(d(&s), deckcraft_geom::squircle::path_d(200.0, 100.0, 16.667, 0.6));
    // Other shapes are refused.
    s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [0, 0, 50, 50]})).unwrap();
    assert!(s.execute("shape.smoothCorners", &json!({"smoothing": 50})).is_err());
    // So is a freeform, whatever radius is given.
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [0, 0, 50, 50]})).unwrap();
    s.execute("shape.editPoints", &json!({"on": true})).unwrap();
    s.execute("shape.editPoints", &json!({"on": false})).unwrap();
    assert!(matches!(s.doc().unwrap().shapes().last().unwrap().geom, deckcraft_model::Geom::Custom { .. }), "Edit Points made a freeform");
    assert!(s.execute("shape.smoothCorners", &json!({"smoothing": 50, "radius": 8})).is_err());
    // The outline round-trips through PPTX as custom geometry.
    let bytes = crate::cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap();
    let back = crate::cmd::file::open_presentation("x.pptx", &bytes).unwrap();
    let sh = back.slides.iter().flat_map(|sl| sl.shapes.iter()).find(|sh| matches!(sh.geom, deckcraft_model::Geom::Custom { .. }));
    assert!(sh.is_some(), "a custom outline in the saved file");
}

#[test]
fn wordart_adjust_handle_drags_and_undoes() {
    let mut s = session();
    s.prefs.snap_to_grid = false;
    s.prefs.smart_guides = false;
    let id =
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 60.0, 420.0, 200.0], "text": "ARCHED"})).unwrap()["id"].as_u64().unwrap();
    let sid = model::ShapeId(id as u32);
    {
        let st = s.doc_mut().unwrap();
        let slide = st.selection.slide;
        let mut doc = (*st.doc).clone();
        let sh = model::find_shape_mut(&mut std::sync::Arc::make_mut(&mut doc.slides[slide]).shapes, sid).unwrap();
        sh.text.as_mut().unwrap().body.warp = Some("textArchUp".into());
        st.doc = std::sync::Arc::new(doc);
    }
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let (at, top) = {
        let st = s.doc().unwrap();
        let sh = st.shape(sid).unwrap();
        let x = cmd::xfrm_of(&st.doc, &st.selection, sh);
        let hs = tools::wordart_handles(&st.doc, &st.selection, sh, x.w, x.h);
        assert_eq!(hs.len(), 1);
        let at = x.affine() * hs[0].pos;
        (at, deckcraft_geom::Point::new(x.center().x, x.y))
    };
    assert!(matches!(s.hit_test(at, 4.0), Some(tools::Hit::Warp { index: 0, .. })));
    let ev = |kind, p: deckcraft_geom::Point| PointerEvent { kind, x: p.x, y: p.y, mods: Mods::default(), tol: 4.0, pressure: None };
    s.pointer(ev(PointerKind::Down, at)).unwrap();
    s.pointer(ev(PointerKind::Drag, top)).unwrap();
    s.pointer(ev(PointerKind::Up, top)).unwrap();
    let adj = s.doc().unwrap().shape(sid).unwrap().text.as_ref().unwrap().body.warp_adj.clone();
    let deg = adj.first().copied().unwrap_or(0.0) / 60_000.0;
    assert!((deg - 270.0).abs() < 2.0, "{deg} from {adj:?}");
    s.execute("edit.undo", &json!({})).unwrap();
    let body = &s.doc().unwrap().shape(sid).unwrap().text.as_ref().unwrap().body;
    assert_eq!(body.warp.as_deref(), Some("textArchUp"));
    assert!(body.warp_adj.is_empty(), "undo restores the default adjusts");
}

#[test]
fn shape_picture_fill_options_tile_offset_and_round_trip() {
    use deckcraft_model::style::PictureMode;
    let mut img = image::RgbaImage::new(4, 4);
    img.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    let pf = |s: &Session| match s.doc().unwrap().selected_shapes()[0].fill.clone() {
        Some(deckcraft_model::Fill::Picture(pf)) => pf,
        other => panic!("{other:?}"),
    };
    // Options without a picture fill are a no-op, not an error.
    s.execute("shape.fill", &json!({"pictureProps": {"tile": true}})).unwrap();
    s.execute("shape.fill", &json!({"picture": cmd::base64_encode(&png)})).unwrap();
    s.execute("shape.fill", &json!({"pictureProps": {"stretch": [0.1, 0.2, 5, "x"]}})).unwrap();
    assert_eq!(pf(&s).mode, PictureMode::Stretch { fill_rect: [0.1, 0.2, 0.99, 0.0] });
    s.execute("shape.fill", &json!({"pictureProps": {"tile": true, "offsetX": 12, "offsetY": -3, "scaleX": 0.5, "scaleY": 50, "align": "ctr", "mirror": "xy", "rotateWithShape": false}})).unwrap();
    s.execute("shape.fill", &json!({"pictureProps": {"align": "bogus", "offsetX": f64::NAN}, "transparency": 0.4})).unwrap();
    let want = PictureMode::Tile { tx: 12.0, ty: -3.0, sx: 0.5, sy: 10.0, flip: "xy".into(), align: "ctr".into() };
    let got = pf(&s);
    assert_eq!(got.mode, want);
    assert!(got.upright);
    assert!((got.alpha.unwrap() - 0.6).abs() < 1e-9);
    // PPTX round trip keeps the options.
    let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap();
    s.execute("file.openBytes", &json!({"name": "p.pptx", "data": cmd::base64_encode(&bytes)})).unwrap();
    let doc = &s.doc().unwrap().doc;
    let back = doc.slides.iter().flat_map(|sl| sl.shapes.iter()).find_map(|sh| match &sh.fill {
        Some(deckcraft_model::Fill::Picture(pf)) => Some(pf.clone()),
        _ => None,
    });
    let back = back.unwrap();
    assert_eq!(back.mode, want);
    assert!(back.upright);
    assert!((back.alpha.unwrap() - 0.6).abs() < 1e-6);
}

#[test]
fn find_and_replace_across_shapes_and_notes() {
    let mut s = session();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Old plan, old team\nboldly"})).unwrap();
    s.execute("slide.notes", &json!({"text": "Discuss the old plan"})).unwrap();
    let q = json!({"text": "old"});
    let found = s.execute("text.find", &q).unwrap();
    let n = found.as_array().unwrap().len();
    assert!(n >= 3, "{found}");
    let whole = s.execute("text.find", &json!({"text": "old", "wholeWords": true, "matchCase": true})).unwrap();
    assert_eq!(whole.as_array().unwrap().len(), n - 2, "Old and boldly are not whole lower-case matches");

    let first = found[0].clone();
    assert_eq!((first["para"].as_u64(), first["start"].as_u64()), (Some(0), Some(0)));
    s.execute("text.replace", &json!({"text": "old", "with": "New", "at": first})).unwrap();
    assert_eq!(s.execute("text.find", &q).unwrap().as_array().unwrap().len(), n - 1);

    let r = s.execute("text.replace", &json!({"text": "old", "with": "new", "wholeWords": true})).unwrap();
    assert_eq!(r["replaced"].as_u64(), Some(n as u64 - 2));
    let text = s.doc().unwrap().doc.slides[0].shapes.iter().filter_map(|x| x.text.as_ref()).map(|t| t.text()).collect::<Vec<_>>().join("|");
    assert!(text.contains("New plan, new team\nboldly"), "{text}");
    assert!(s.execute("text.replace", &json!({"text": "zebra", "with": "x"})).is_err());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.execute("text.find", &q).unwrap().as_array().unwrap().len(), n - 1);
}

/// Find and Replace count a line break as one character, so offsets agree with the editor and no
/// match spans the break.
#[test]
fn find_and_replace_after_a_line_break() {
    let mut s = session();
    let id = s.execute("insert.textBox", &json!({"rect": [10, 10, 300, 80], "text": "ab"})).unwrap()["id"].as_u64().unwrap();
    s.execute("text.edit", &json!({"id": id, "end": true})).unwrap();
    s.execute("text.insert", &json!({"text": "\u{b}cd"})).unwrap();
    s.execute("text.exit", &json!({})).unwrap();
    let text = |s: &Session| {
        s.doc()
            .unwrap()
            .current_slide()
            .unwrap()
            .shapes
            .iter()
            .find(|x| u64::from(x.id.0) == id)
            .and_then(|x| x.text.as_ref())
            .map(|t| t.text())
            .unwrap()
    };
    assert_eq!(text(&s), "ab\u{b}cd");
    assert!(s.execute("text.find", &json!({"text": "bc"})).unwrap().as_array().unwrap().is_empty(), "no match spans the break");
    let found = s.execute("text.find", &json!({"text": "cd"})).unwrap();
    assert_eq!((found[0]["start"].as_u64(), found[0]["end"].as_u64()), (Some(3), Some(5)), "{found}");
    s.execute("text.replace", &json!({"text": "cd", "with": "X"})).unwrap();
    assert_eq!(text(&s), "ab\u{b}X");
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&s), "ab\u{b}cd");
}

#[test]
fn pen_pressure_is_kept_and_a_mouse_draws_at_full_width() {
    let mut s = session();
    s.set_tool(tools::ToolKind::Ink { mode: "pen".into(), color: deckcraft_model::Rgba::rgb(20, 20, 20), width: 4.0 });
    let ev = |kind, x, y, pressure| tools::PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure };
    s.pointer(ev(PointerKind::Down, 40.0, 80.0, Some(0.2))).unwrap();
    s.pointer(ev(PointerKind::Drag, 80.0, 80.0, Some(1.0))).unwrap();
    s.pointer(ev(PointerKind::Drag, 120.0, 80.0, Some(0.4))).unwrap();
    s.pointer(ev(PointerKind::Up, 120.0, 80.0, Some(0.4))).unwrap();
    s.pointer(ev(PointerKind::Down, 40.0, 160.0, None)).unwrap();
    s.pointer(ev(PointerKind::Drag, 100.0, 160.0, Some(f32::NAN))).unwrap();
    s.pointer(ev(PointerKind::Up, 100.0, 160.0, None)).unwrap();
    let got = ink_pressures(&s);
    assert_eq!(got, vec![vec![0.2, 1.0, 0.4], vec![1.0, 1.0]]);
}

fn ink_pressures(s: &Session) -> Vec<Vec<f32>> {
    let st = s.doc().unwrap();
    let Some(sh) = st.shapes().iter().rev().find(|sh| matches!(sh.kind, deckcraft_model::ShapeKind::Ink { .. })) else {
        return vec![];
    };
    let deckcraft_model::ShapeKind::Ink { strokes } = &sh.kind else { return vec![] };
    strokes.iter().map(|stroke| stroke.points.iter().map(|p| p.2).collect()).collect()
}

#[test]
fn shape_three_d_format_and_rotation_round_trip() {
    use crate::cmd::three_d::Shape3d;
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    let fx = |s: &Session| s.doc().unwrap().selected_shapes()[0].effects.clone().unwrap_or_default();
    // The old `bevel` key now writes real 3-D XML, so it is drawn and saved.
    s.execute("shape.effects", &json!({"bevel": "angle"})).unwrap();
    let e = fx(&s);
    assert_eq!(e.bevel.as_deref(), Some("angle"));
    assert!(e.raw3d.as_deref().is_some_and(|r| r.contains("bevelT") && r.contains("prst=\"angle\"")));
    s.execute(
        "shape.effects",
        &json!({"threeD": {"bevelTopW": 9, "bevelBottom": "circle", "depth": 20, "contour": 2, "material": "metal",
            "lighting": "harsh", "camera": "isometricTopUp", "rotX": 30, "rotY": -20, "z": 5, "bogus": 1}}),
    )
    .unwrap();
    // Unknown names and non-finite numbers are ignored.
    s.execute("shape.effects", &json!({"threeD": {"material": "gold", "depth": f64::NAN, "camera": 7}})).unwrap();
    let want = fx(&s).raw3d.map(|r| Shape3d::parse(&r)).unwrap();
    assert_eq!(want.top.as_ref().map(|b| (b.preset.as_str(), b.w, b.h)), Some(("angle", 9.0, 6.0)));
    assert_eq!(want.bottom.as_ref().map(|b| b.preset.as_str()), Some("circle"));
    assert_eq!((want.depth, want.contour, want.z), (20.0, 2.0, 5.0));
    assert_eq!((want.material.as_str(), want.light.as_str(), want.camera.as_str()), ("metal", "harsh", "isometricTopUp"));
    assert_eq!(want.rot, Some((30.0, 340.0, 0.0)));
    // PPTX save and reopen keeps every setting.
    let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, "pptx").unwrap();
    s.execute("file.openBytes", &json!({"name": "d.pptx", "data": cmd::base64_encode(&bytes)})).unwrap();
    let back = s.doc().unwrap().doc.slides.iter().flat_map(|sl| sl.shapes.iter()).find_map(|sh| sh.effects.as_ref()?.raw3d.clone());
    assert_eq!(Shape3d::parse(&back.unwrap()), want);
}

#[test]
fn three_d_reset_and_flat_shapes_drop_the_xml() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    let fx = |s: &Session| s.doc().unwrap().selected_shapes()[0].effects.clone().unwrap_or_default();
    s.execute("shape.effects", &json!({"threeD": {"rotZ": 15, "depth": 4}})).unwrap();
    assert!(fx(&s).raw3d.is_some());
    s.execute("shape.effects", &json!({"threeD": {"reset": true}})).unwrap();
    assert_eq!((fx(&s).raw3d, fx(&s).bevel), (None, None));
    // Keeps colours it doesn't model.
    let raw = "<a:scene3d><a:camera prst=\"orthographicFront\"/><a:lightRig rig=\"threePt\" dir=\"t\"/></a:scene3d><a:sp3d extrusionH=\"12700\"><a:extrusionClr><a:srgbClr val=\"FF0000\"/></a:extrusionClr></a:sp3d>";
    let mut s3 = cmd::three_d::Shape3d::parse(raw);
    s3.apply(&json!({"depth": 3}));
    assert!(s3.to_xml().unwrap().contains("<a:extrusionClr><a:srgbClr val=\"FF0000\"/></a:extrusionClr>"));
}

/// A presentation read off the UI thread enters the session through `file.adopt`: journaled like
/// `file.open`, and replayable from the journal without the staged copy.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn background_loads_are_adopted_through_a_command() {
    let mut src = Session::with_new();
    src.execute("slide.new", &json!({})).unwrap();
    let path = std::env::temp_dir().join(format!("deckcraft-adopt-{}.deckcraft", std::process::id()));
    let path = path.to_string_lossy().to_string();
    src.execute("file.saveAs", &json!({"path": path})).unwrap();

    let mut s = Session::new();
    let loaded = cmd::file::read_presentation_file(&path).unwrap();
    let token = s.stage_loaded(loaded);
    let r = s.execute("file.adopt", &json!({"path": path, "token": token})).unwrap();
    assert_eq!(r["slides"], 2, "{r}");
    assert_eq!(s.documents().len(), 1);
    assert_eq!(s.doc().unwrap().path.as_deref(), Some(path.as_str()));
    let (id, params) = s.journal.last().cloned().unwrap();
    assert_eq!(id, "file.adopt");
    // The token was used up; replaying the entry reads the file again.
    let mut replay = Session::new();
    replay.execute(&id, &params).unwrap();
    assert_eq!(replay.doc().unwrap().doc.slides.len(), 2);
    // A token staged for another path is not taken for this one.
    let other =
        s.stage_loaded(cmd::file::LoadedFile { path: "/elsewhere.pptx".into(), doc: Default::default(), native: true, damaged: Default::default() });
    s.execute("file.adopt", &json!({"path": path, "token": other})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 2);
    assert!(s.execute("file.adopt", &json!({})).is_err());
    std::fs::remove_file(&path).ok();
}

#[test]
fn every_command_id_is_registered_once() {
    let mut seen = std::collections::HashSet::new();
    let dups: Vec<&str> = crate::command_specs().iter().map(|c| c.id).filter(|id| !seen.insert(*id)).collect();
    assert!(dups.is_empty(), "registered more than once: {dups:?}");
}

/// A deck whose ids are used up (a hostile file can arrive with next_id at the top of the
/// range) refuses to grow instead of handing out duplicate ids (#407).
#[test]
fn an_exhausted_id_space_refuses_growth() {
    let mut s = session();
    s.execute("slide.new", &json!({})).unwrap();
    std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc).next_id = u32::MAX;
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("slide.new", &json!({})).is_err(), "a new slide is refused");
    assert!(s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 100, 50]})).is_err());
    assert!(s.execute("slide.duplicate", &json!({})).is_err());
    assert!(s.execute("edit.duplicate", &json!({})).is_err());
    assert!(s.execute("insert.smartArt", &json!({"kind": "blockList", "items": ["Plan", "Build"]})).is_err());
    assert!(s.execute("insert.textBox", &json!({"text": "Hi"})).is_err());
    // What the deck holds is untouched: same slides, same ids, no duplicates.
    let after = s.doc().unwrap().doc.clone();
    assert_eq!(after.slides.len(), before.slides.len());
    let mut ids = vec![];
    deckcraft_model::walk(&after.slides[0].shapes, &mut |sh, _| ids.push(sh.id.0));
    ids.push(after.slides[0].id.0);
    ids.push(after.next_id);
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "no id repeats");
}

#[test]
fn shape_ids_beyond_u32_are_rejected_not_wrapped() {
    let (mut s, a, b) = two_shapes();
    let wrap = |id: deckcraft_model::ShapeId| u64::from(id.0) + (1u64 << 32);
    let shapes_before = s.doc().unwrap().shapes().len();
    let r = s.execute("shape.connect", &json!({"from": wrap(a), "to": wrap(b)}));
    assert!(r.is_err(), "connect must reject ids beyond u32, got {r:?}");
    let r = s.execute("shape.sites", &json!({"id": wrap(a)}));
    assert!(r.is_err(), "sites must reject ids beyond u32, got {r:?}");
    let r = s.execute("shape.points", &json!({"id": wrap(a)}));
    assert!(r.is_err(), "points must reject ids beyond u32, got {r:?}");
    let r = s.execute("shape.movePoint", &json!({"id": wrap(a), "index": 0, "x": 1.0, "y": 1.0}));
    assert!(r.is_err_and(|e| e.to_string().contains("not a valid shape id")), "movePoint must reject ids beyond u32");
    assert_eq!(s.doc().unwrap().shapes().len(), shapes_before, "no connector was created");
    // In-range ids still work.
    assert!(s.execute("shape.sites", &json!({"id": a.0})).is_ok());
    assert!(s.execute("shape.connect", &json!({"from": a.0, "to": b.0})).is_ok());
}

#[test]
fn table_insert_below_or_right_of_a_huge_index_appends_at_the_end() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0], json!([["a", "b"], ["c", "d"]]));
    let r = s.execute("table.insertRowBelow", &json!({"id": id.0, "row": u64::MAX}));
    assert!(r.is_ok(), "insertRowBelow must clamp a huge row, got {r:?}");
    let t = table_of(&s, id);
    assert_eq!(t.n_rows(), 3);
    assert_eq!(t.cell(0, 0).unwrap().text.text(), "a", "the original rows keep their place");
    let r = s.execute("table.insertColumnRight", &json!({"id": id.0, "col": u64::MAX}));
    assert!(r.is_ok(), "insertColumnRight must clamp a huge col, got {r:?}");
    let t = table_of(&s, id);
    assert_eq!(t.n_cols(), 3);
    assert_eq!(t.cell(0, 1).unwrap().text.text(), "b", "the original columns keep their place");
}

#[test]
fn table_column_width_of_a_huge_column_is_ignored() {
    let mut s = session();
    let id = table_at(&mut s, &[100.0, 100.0], json!([["a", "b"], ["c", "d"]]));
    for keep_total in [true, false] {
        let r = s.execute("table.columnWidth", &json!({"id": id.0, "col": u64::MAX, "width": 50.0, "keepTotal": keep_total}));
        assert!(r.is_ok(), "columnWidth must ignore a huge col (keepTotal {keep_total}), got {r:?}");
        assert_eq!(table_of(&s, id).cols, vec![100.0, 100.0], "no column changes");
    }
}

/// Slides imported from an outline join the section of the slide they follow, like `slide.new`.
#[test]
fn slides_from_outline_join_the_section_of_the_slide_they_follow() {
    for markdown in [false, true] {
        let mut s = session();
        s.execute("slide.new", &json!({"title": "A"})).unwrap();
        s.execute("section.add", &json!({"name": "B", "at": 1})).unwrap();
        let text = if markdown { "# Imported\n- bullet" } else { "Imported\n\tbullet" };
        s.execute("slide.fromOutline", &json!({"text": text, "markdown": markdown})).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        let orphans: Vec<_> = doc.slides.iter().map(|x| x.id).filter(|id| doc.section_of(*id).is_none()).collect();
        assert!(orphans.is_empty(), "markdown={markdown}: slides in no section: {orphans:?}; sections {:?}", doc.sections);
        let order: Vec<_> = doc.sections.iter().flat_map(|x| x.slides.iter().copied()).collect();
        let slides: Vec<_> = doc.slides.iter().map(|x| x.id).collect();
        assert_eq!(order, slides, "markdown={markdown}: sections follow the slide order");
    }
}

#[test]
fn opening_a_pptx_with_an_unreadable_slide_says_so() {
    use std::io::{Cursor, Read, Write};
    let mut s = session();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    assert_eq!(s.doc().unwrap().doc.slides.len(), 2);
    let good = deckcraft_pptx::export(s.doc().unwrap().doc.as_ref()).unwrap();
    let mut zin = zip::ZipArchive::new(Cursor::new(good.clone())).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).unwrap();
        let mut d = vec![];
        f.read_to_end(&mut d).unwrap();
        if f.name() == "ppt/slides/slide2.xml" {
            d = b"<p:sld".to_vec();
        }
        out.start_file(f.name(), o).unwrap();
        out.write_all(&d).unwrap();
    }
    let broken = out.finish().unwrap().into_inner();

    let mut t = session();
    let r = t.execute("file.openBytes", &json!({"name": "Broken.pptx", "data": cmd::base64_encode(&broken)})).unwrap();
    assert_eq!(r["lostSlides"], json!(["ppt/slides/slide2.xml"]), "{r}");
    assert_eq!(t.doc().unwrap().doc.slides.len(), 1);
    assert!(t.doc().unwrap().title.ends_with("(Repaired)"), "{}", t.doc().unwrap().title);
    assert!(t.ui_requests.iter().any(|u| matches!(u, crate::UiRequest::Message { text } if text.contains("1 slide in this file couldn't be read"))));
    // A good file reports nothing.
    let mut u = session();
    let r = u.execute("file.openBytes", &json!({"name": "Fine.pptx", "data": cmd::base64_encode(&good)})).unwrap();
    assert_eq!(r["lostSlides"], json!([]), "{r}");
    assert!(u.ui_requests.is_empty());
}

/// A file with absurd effect sizes opens with them capped at what Format Shape allows: drawn as
/// they were, a glow of 10^18 pt made the renderer ask for 34 GB and abort.
#[test]
fn opening_a_file_caps_absurd_effect_sizes() {
    use deckcraft_model::style::{Glow, Reflection};
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100], "text": "Glow"})).unwrap();
    s.execute("shape.effects", &json!({"shadow": "outer", "reflection": "tight", "softEdges": 4, "glow": {"radius": 8}})).unwrap();
    s.execute("insert.table", &json!({"rows": 1, "cols": 1, "rect": [400, 300, 100, 40]})).unwrap();
    let huge = 1e18;
    s.edit(|doc, sel| {
        let list = crate::shapes_mut(doc, sel).unwrap();
        for sh in list.iter_mut() {
            if let Some(e) = sh.effects.as_mut() {
                e.glow.as_mut().unwrap().radius = huge;
                e.soft_edge = Some(f64::NAN);
                let sw = e.outer_shadow.as_mut().unwrap();
                (sw.blur, sw.dist, sw.sx) = (huge, huge, huge);
                *e.reflection.as_mut().unwrap() = Reflection { blur: huge, start_alpha: 0.5, end_alpha: 0.0, end_pos: 0.5, dist: huge };
            }
            let glow = Some(Glow { color: deckcraft_model::style::ColorRef::rgb(deckcraft_color::Rgba::BLACK), radius: huge });
            let runs: Vec<&mut deckcraft_model::text::Run> = match &mut sh.kind {
                deckcraft_model::ShapeKind::Table(t) => t
                    .rows
                    .iter_mut()
                    .flat_map(|r| r.cells.iter_mut())
                    .flat_map(|c| c.text.paragraphs.iter_mut())
                    .flat_map(|p| p.runs.iter_mut())
                    .collect(),
                _ => sh.text.iter_mut().flat_map(|t| t.paragraphs.iter_mut()).flat_map(|p| p.runs.iter_mut()).collect(),
            };
            for r in runs {
                r.props.glow = glow.clone();
            }
        }
        // Inherited styles too: the deck default, a master's body style, a text box's own list
        // style, and the theme's effect styles that shape styles point at.
        let level = deckcraft_model::text::LevelStyle {
            run: deckcraft_model::text::RunProps {
                glow: Some(Glow { color: deckcraft_model::style::ColorRef::rgb(deckcraft_color::Rgba::BLACK), radius: huge }),
                ..Default::default()
            },
            ..Default::default()
        };
        doc.default_text_style.set(0, level.clone());
        let m = std::sync::Arc::make_mut(&mut doc.masters[0]);
        m.body_style.set(0, level.clone());
        if let Some(fx) = m.theme.format.effects.first_mut() {
            fx.glow = Some(Glow { color: deckcraft_model::style::ColorRef::rgb(deckcraft_color::Rgba::BLACK), radius: huge });
        }
        let list = crate::shapes_mut(doc, sel).unwrap();
        if let Some(t) = list.iter_mut().find_map(|sh| sh.text.as_mut()) {
            t.list_style.set(0, level);
        }
        Ok(())
    })
    .unwrap();
    for format in ["deckcraft", "pptx"] {
        let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, format).unwrap();
        let mut t = Session::new();
        t.execute("file.openBytes", &json!({"name": format!("fx.{format}"), "data": cmd::base64_encode(&bytes)})).unwrap();
        let st = t.doc().unwrap();
        let slide = st.doc.slides.last().unwrap();
        let mut glows = vec![];
        deckcraft_model::walk(&slide.shapes, &mut |sh, _| {
            if let Some(e) = &sh.effects {
                glows.extend(e.glow.as_ref().map(|g| g.radius));
                assert!(e.soft_edge.is_none_or(|v| (0.0..=100.0).contains(&v)), "{format}: {:?}", e.soft_edge);
                if let Some(sw) = &e.outer_shadow {
                    assert!(sw.blur <= 200.0 && sw.dist <= 200.0 && sw.sx <= 10.0, "{format}: {sw:?}");
                }
                if let Some(r) = &e.reflection {
                    assert!(r.blur <= 100.0 && r.dist <= 100.0, "{format}: {r:?}");
                }
            }
            for p in sh.text.iter().flat_map(|t| t.paragraphs.iter()) {
                glows.extend(p.runs.iter().filter_map(|r| r.props.glow.as_ref().map(|g| g.radius)));
            }
            if let deckcraft_model::ShapeKind::Table(tb) = &sh.kind {
                let cells = tb.rows.iter().flat_map(|r| r.cells.iter()).flat_map(|c| c.text.paragraphs.iter());
                glows.extend(cells.flat_map(|p| p.runs.iter()).filter_map(|r| r.props.glow.as_ref().map(|g| g.radius)));
            }
        });
        let inherited = |ls: &deckcraft_model::text::ListStyle| {
            ls.levels.iter().flatten().filter_map(|l| l.run.glow.as_ref().map(|g| g.radius)).collect::<Vec<_>>()
        };
        glows.extend(inherited(&st.doc.default_text_style));
        let m = &st.doc.masters[0];
        for ls in [&m.title_style, &m.body_style, &m.other_style] {
            glows.extend(inherited(ls));
        }
        glows.extend(m.theme.format.effects.iter().filter_map(|e| e.glow.as_ref().map(|g| g.radius)));
        deckcraft_model::walk(&slide.shapes, &mut |sh, _| glows.extend(sh.text.iter().flat_map(|t| inherited(&t.list_style))));
        assert!(!glows.is_empty(), "{format}: the glows were kept");
        assert!(glows.iter().all(|g| (0.0..=150.0).contains(g)), "{format}: {glows:?}");
        // Capped, the slide renders (checked last: uncapped, this is what ran out of memory).
        let i = st.doc.slides.len() - 1;
        t.execute("file.render", &json!({"slide": i, "scale": 0.1})).unwrap();
    }
}

/// A .deckcraft file whose shapes repeat an id (groups included) opens with fresh ids for the
/// repeats: the deck passes `validate`, the first shape keeps its id and none is lost.
#[test]
fn opening_a_file_gives_repeated_shape_ids_fresh_ones() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    for x in [0, 100, 200, 300] {
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [x, 50, 80, 80]})).unwrap();
    }
    s.execute("edit.selectAll", &json!({})).unwrap();
    s.execute("arrange.group", &json!({})).unwrap();
    s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [400, 300, 80, 80]})).unwrap();
    let first = s.doc().unwrap().shapes()[0].id;
    // Every shape, the group's children too, takes the group's id.
    s.edit(|doc, sel| {
        fn all(shapes: &mut [deckcraft_model::Shape], id: deckcraft_model::ShapeId) {
            for sh in shapes {
                sh.id = id;
                if let deckcraft_model::ShapeKind::Group { children, .. } = &mut sh.kind {
                    all(children, id);
                }
            }
        }
        all(crate::shapes_mut(doc, sel).unwrap(), first);
        Ok(())
    })
    .unwrap();
    assert!(!s.doc().unwrap().doc.validate().is_empty(), "the deck repeats ids");
    let bytes = cmd::file::save_bytes(&s.doc().unwrap().doc, "deckcraft").unwrap();
    let mut t = Session::new();
    t.execute("file.openBytes", &json!({"name": "dup.deckcraft", "data": cmd::base64_encode(&bytes)})).unwrap();
    let st = t.doc().unwrap();
    assert_eq!(st.doc.validate(), Vec::<String>::new());
    let slide = st.doc.slides.last().unwrap();
    let mut ids = vec![];
    deckcraft_model::walk(&slide.shapes, &mut |sh, _| ids.push(sh.id));
    assert_eq!(ids.len(), 6, "the group, its four children and the ellipse");
    assert_eq!(ids[0], first, "the first shape keeps its id");
    assert!(ids.iter().all(|id| id.0 <= st.doc.next_id), "new ids come from the deck's counter");
    // Each shape can be selected by its own id now.
    let last = *ids.last().unwrap();
    t.execute("slide.go", &json!({"index": st.doc.slides.len() - 1})).unwrap();
    t.execute("selection.select", &json!({"id": last.0})).unwrap();
    assert_eq!(t.doc().unwrap().selected_shapes()[0].id, last);
}

#[test]
fn change_case_spans_formatting_runs() {
    let setup = |text: &str, bold_to: usize| {
        let mut s = session();
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [80.0, 80.0, 420.0, 160.0], "text": text})).unwrap();
        let id = r["id"].as_u64().unwrap() as u32;
        s.execute("text.edit", &json!({"id": id})).unwrap();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, bold_to]})).unwrap();
        s.execute("format.bold", &json!({"on": true})).unwrap();
        (s, id)
    };
    let text_of = |s: &Session, id: u32| s.doc().unwrap().shape(deckcraft_model::ShapeId(id)).unwrap().text.clone().unwrap();
    for (mode, input, want) in [("title", "hello world", "Hello World"), ("sentence", "hello world. bye", "Hello world. Bye")] {
        // Text range.
        let (mut s, id) = setup(input, 3);
        let n = input.chars().count();
        s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, n]})).unwrap();
        s.execute("format.case", &json!({"mode": mode})).unwrap();
        assert_eq!(text_of(&s, id).text(), want, "{mode} range");
        // Whole shape.
        let (mut s, id) = setup(input, 3);
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("format.case", &json!({"mode": mode})).unwrap();
        assert_eq!(text_of(&s, id).text(), want, "{mode} shape");
        assert_eq!(text_of(&s, id).paragraphs[0].runs[0].props.bold, Some(true));
    }
    // A length-changing mapping keeps each piece in its own run.
    let (mut s, id) = setup("straße", 4);
    s.execute("text.select", &json!({"anchor": [0, 0], "caret": [0, 6]})).unwrap();
    s.execute("format.case", &json!({"mode": "upper"})).unwrap();
    let body = text_of(&s, id);
    assert_eq!(body.text(), "STRASSE");
    assert_eq!(body.paragraphs[0].runs[0].text, "STRA");
    assert_eq!(body.paragraphs[0].runs[0].props.bold, Some(true));
}

#[test]
fn flipping_a_rotated_shape_mirrors_its_outline() {
    let corners = |s: &Session, id: deckcraft_model::ShapeId| {
        let x = s.doc().unwrap().shape(id).unwrap().xfrm.unwrap();
        let a = x.affine();
        [(0.0, 0.0), (x.w, 0.0), (x.w, x.h), (0.0, x.h)].map(|(px, py)| {
            let p = a * deckcraft_geom::Point::new(px, py);
            ((p.x * 100.0).round() as i64, (p.y * 100.0).round() as i64)
        })
    };
    let sorted = |mut c: [(i64, i64); 4]| {
        c.sort_unstable();
        c
    };
    for (cmd, horizontal) in [("arrange.flipHorizontal", true), ("arrange.flipVertical", false)] {
        let mut s = session();
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 50]})).unwrap();
        let id = deckcraft_model::ShapeId(r["id"].as_u64().unwrap() as u32);
        s.execute("shape.rotate", &json!({"deg": 30})).unwrap();
        let before = corners(&s, id);
        // Mirror about the box's centre line (centre is 200, 125).
        let mirrored = before.map(|(x, y)| if horizontal { (40000 - x, y) } else { (x, 25000 - y) });
        s.execute(cmd, &json!({})).unwrap();
        assert_eq!(sorted(corners(&s, id)), sorted(mirrored), "{cmd} of a 30° rotated rectangle is not its mirror image");
        let rot = s.doc().unwrap().shape(id).unwrap().xfrm.unwrap().rot;
        assert!((rot - 330.0).abs() < 1e-9, "{cmd} negates the rotation, got {rot}");
        s.execute(cmd, &json!({})).unwrap();
        assert_eq!(sorted(corners(&s, id)), sorted(before), "{cmd} twice restores the outline");
    }
}

#[test]
fn shape_geometry_commands_limit_size_and_position() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "moon", "rect": [100, 100, 200, 100]})).unwrap();
    let xfrm = |s: &Session| s.doc().unwrap().selected_shapes()[0].xfrm.unwrap();
    let within = |s: &Session| {
        let x = xfrm(s);
        [x.w, x.h].iter().all(|v| v.is_finite() && (0.0..=15_840.0).contains(v)) && [x.x, x.y].iter().all(|v| v.is_finite() && v.abs() <= 100_000.0)
    };
    s.execute("shape.setBounds", &json!({"x": 1e100, "y": -1e100, "w": 100000, "h": 1e100})).unwrap();
    let x = xfrm(&s);
    assert!(within(&s), "setBounds stored {x:?}");
    s.execute("shape.resize", &json!({"w": 10, "h": 10})).unwrap();
    s.execute("shape.resize", &json!({"h": 1e100})).unwrap();
    let x = xfrm(&s);
    assert!(within(&s), "resize stored {x:?}");
    s.execute("shape.resize", &json!({"h": 1e100, "lockAspect": true})).unwrap();
    let x = xfrm(&s);
    assert!(within(&s), "locked resize stored {x:?}");
    s.execute("shape.move", &json!({"x": 1e100, "y": -1e100})).unwrap();
    let x = xfrm(&s);
    assert!(within(&s), "move stored {x:?}");
    // A move that overflows to infinity lands on the limit, not back at 0.
    s.execute("shape.move", &json!({"x": 1e308, "dx": 1e308})).unwrap();
    assert_eq!(xfrm(&s).x, 100_000.0);
    // A rotated shape resizes about its centre and still stays inside the limit.
    s.execute("shape.setBounds", &json!({"x": 99_000, "y": 0, "w": 10, "h": 10})).unwrap();
    s.execute("shape.rotate", &json!({"deg": 45})).unwrap();
    s.execute("shape.resize", &json!({"w": 1e100})).unwrap();
    let x = xfrm(&s);
    assert!(within(&s), "rotated resize stored {x:?}");
}

/// Edit Points turns a preset into a freeform: one undo step, so Undo brings the preset back
/// (and leaves Edit Points), Redo the freeform; leaving Edit Points adds no step.
#[test]
fn edit_points_conversion_is_one_undo_step() {
    use deckcraft_model::Geom;
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "star5", "rect": [100, 100, 200, 100]})).unwrap();
    let preset = |s: &Session| matches!(s.doc().unwrap().selected_shapes()[0].geom, Geom::Preset { .. });
    s.execute("shape.editPoints", &json!({"on": true})).unwrap();
    assert!(!preset(&s), "Edit Points made a freeform");
    assert!(s.tool.editing_points(s.doc().unwrap()).is_some());
    s.execute("shape.editPoints", &json!({"on": false})).unwrap();
    // Leaving changed nothing, so the next undo is the conversion itself.
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(preset(&s), "Undo brings the star back");
    s.execute("edit.redo", &json!({})).unwrap();
    assert!(!preset(&s), "Redo makes it a freeform again");
    // Undo while still editing points ends Edit Points: the star has no points to edit.
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("shape.editPoints", &json!({"on": true})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(preset(&s));
    assert!(s.tool.editing_points(s.doc().unwrap()).is_none());
    // A freeform needs no conversion: entering Edit Points again adds no undo step.
    s.execute("edit.redo", &json!({})).unwrap();
    s.execute("shape.editPoints", &json!({"on": false})).unwrap();
    s.execute("shape.editPoints", &json!({"on": true})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(preset(&s), "the only step was the conversion");
}

/// #931: while the text of a placeholder is being edited, a shape in front of it still takes a
/// click (as in PowerPoint); a click on the text where nothing is in front still moves the caret.
#[test]
fn a_shape_in_front_of_edited_text_takes_the_click() {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "titleAndContent"})).unwrap();
    let square = s.execute("shape.insert", &json!({"preset": "rect", "rect": [380, 260, 200, 120]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.deselect", &json!({})).unwrap();
    let ev = |kind, x: f64, y: f64| tools::PointerEvent { kind, x, y, mods: Default::default(), tol: 4.0, pressure: None };
    let click = |s: &mut Session, x: f64, y: f64| {
        s.pointer(ev(tools::PointerKind::Down, x, y)).unwrap();
        s.pointer(ev(tools::PointerKind::Up, x, y)).unwrap();
    };
    // The content placeholder's edge: its (empty) text is now being edited.
    click(&mut s, 66.0, 300.0);
    let body = s.doc().unwrap().selection.text.as_ref().map(|t| t.shape).expect("editing the content placeholder");
    assert_ne!(body.0 as u64, square);
    // A click on the text away from the square keeps editing it.
    click(&mut s, 200.0, 300.0);
    assert_eq!(s.doc().unwrap().selection.text.as_ref().map(|t| t.shape), Some(body));
    // A click on the square, in front of the text, selects the square.
    click(&mut s, 480.0, 320.0);
    let st = s.doc().unwrap();
    assert_eq!(st.selection.shapes.iter().map(|id| id.0 as u64).collect::<Vec<_>>(), vec![square]);
    assert!(st.selection.text.as_ref().is_none_or(|t| t.shape.0 as u64 == square), "{:?}", st.selection.text);

    // A shape over the edited box's edge takes the click there too: the edge would otherwise drag
    // the box.
    let edge = {
        let st = s.doc().unwrap();
        let x = cmd::xfrm_of(&st.doc, &st.selection, st.shape(body).unwrap());
        (x.x, x.y + x.h / 2.0)
    };
    let corner =
        s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [edge.0 - 10.0, edge.1 - 10.0, 20, 20]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.deselect", &json!({})).unwrap();
    s.execute("text.edit", &json!({"id": body.0})).unwrap();
    // On the box's left edge, inside the band that drags it, clear of its middle handle (handles
    // stay on top, as in PowerPoint).
    click(&mut s, edge.0 + 1.0, edge.1 - 9.0);
    let st = s.doc().unwrap();
    assert_eq!(st.selection.shapes.iter().map(|id| id.0 as u64).collect::<Vec<_>>(), vec![corner], "{:?}", st.selection);

    // Text inside a group: a member in front of it takes the click instead of the caret.
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let tb =
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 300, 150], "text": "Grouped text"})).unwrap()["id"].as_u64().unwrap();
    let cover = s.execute("shape.insert", &json!({"preset": "rect", "rect": [200, 150, 60, 40]})).unwrap()["id"].as_u64().unwrap();
    s.execute("arrange.group", &json!({"ids": [tb, cover]})).unwrap();
    s.execute("text.edit", &json!({"id": tb})).unwrap();
    assert_eq!(s.doc().unwrap().selection.text.as_ref().map(|t| t.shape.0 as u64), Some(tb));
    click(&mut s, 230.0, 170.0);
    let st = s.doc().unwrap();
    assert!(st.selection.text.as_ref().is_none_or(|t| t.shape.0 as u64 != tb), "the click on the member in front left the text: {:?}", st.selection);
}

/// A box at `rect` and a second box at `other` on a fresh slide; smart guides on, the grid off.
/// The first box is selected.
fn snap_scene(rect: [f64; 4], other: [f64; 4]) -> (Session, u64) {
    let mut s = session();
    s.prefs.snap_to_grid = false;
    s.execute("shape.insert", &json!({"preset": "rect", "rect": other})).unwrap();
    let id = s.execute("shape.insert", &json!({"preset": "rect", "rect": rect})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    s.prefs.smart_guides = true;
    (s, id)
}

/// The slide box (`x`, `y`, `w`, `h`) of shape `id`, as `shape.inspect` reports it.
fn box_of(s: &mut Session, id: u64) -> serde_json::Value {
    s.execute("shape.inspect", &json!({"id": id})).unwrap()["box"].clone()
}

/// #961: dragging the east handle snaps the dragged (right) edge, even when the fixed left edge
/// already lines up with another shape. The nearest line used to be the fixed edge's, at distance 0.
#[test]
fn resizing_snaps_the_dragged_edge_when_the_fixed_edge_is_already_aligned() {
    // The box spans x 100..200; the other box starts at 100 too and ends at 300.
    let (mut s, id) = snap_scene([100.0, 100.0, 100.0, 80.0], [100.0, 300.0, 200.0, 80.0]);
    drag_gesture(&mut s, (200.0, 140.0), (297.0, 140.0));
    let b = box_of(&mut s, id);
    assert_eq!(b["x"], 100.0, "the fixed edge stays: {b}");
    assert_eq!(b["w"], 200.0, "the right edge snaps to the other box's right edge at 300: {b}");
}

/// #961: only the dragged edge looks for a line. A fixed edge near a line must not shift the
/// edge being dragged by its own distance.
#[test]
fn resizing_is_not_shifted_by_a_fixed_edge_near_a_line() {
    // The other box starts at 103, 3 pt from the fixed left edge; the dragged edge ends far from any line.
    let (mut s, id) = snap_scene([100.0, 100.0, 100.0, 80.0], [103.0, 300.0, 50.0, 80.0]);
    drag_gesture(&mut s, (200.0, 140.0), (421.0, 140.0));
    let b = box_of(&mut s, id);
    assert_eq!(b["x"], 100.0, "the fixed edge stays: {b}");
    assert_eq!(b["w"], 321.0, "the dragged edge goes where the pointer went: {b}");
}

/// #961: dragging a handle past the opposite edge flips the box, and the dragged edge becomes the
/// other side of it: that side snaps, not the fixed one.
#[test]
fn resizing_past_the_opposite_edge_snaps_the_dragged_edge() {
    // East handle of the box at x 100..200, dragged left past its left edge to 3, 3 pt from the
    // other box's left edge at 0.
    let (mut s, id) = snap_scene([100.0, 100.0, 100.0, 80.0], [0.0, 300.0, 50.0, 80.0]);
    drag_gesture(&mut s, (200.0, 140.0), (3.0, 140.0));
    let b = box_of(&mut s, id);
    assert_eq!((b["x"].clone(), b["w"].clone()), (json!(0.0), json!(100.0)), "the dragged edge snaps to 0, the fixed edge stays at 100: {b}");
    // South handle of the box at y 100..180, dragged up past its top edge to 3.
    let (mut s, id) = snap_scene([100.0, 100.0, 100.0, 80.0], [300.0, 0.0, 80.0, 50.0]);
    drag_gesture(&mut s, (150.0, 180.0), (150.0, 3.0));
    let b = box_of(&mut s, id);
    assert_eq!((b["y"].clone(), b["h"].clone()), (json!(0.0), json!(100.0)), "the dragged edge snaps to 0, the fixed edge stays at 100: {b}");
}

/// #961: the handles of a flipped box sit on the other side (the east handle draws on the left),
/// and the edge under the dragged handle snaps.
#[test]
fn resizing_a_flipped_box_snaps_the_dragged_edge() {
    // Flipped box at x 100..200: its east handle is at x 100. Dragged to 3, 3 pt from the other box's left edge at 0.
    let (mut s, id) = snap_scene([100.0, 100.0, 100.0, 80.0], [0.0, 300.0, 50.0, 80.0]);
    s.execute("shape.flip", &json!({"axis": "horizontal"})).unwrap();
    drag_gesture(&mut s, (100.0, 140.0), (3.0, 140.0));
    let b = box_of(&mut s, id);
    assert_eq!((b["x"].clone(), b["w"].clone()), (json!(0.0), json!(200.0)), "the dragged edge snaps to 0, the fixed edge stays at 200: {b}");
}

#[test]
fn duplicating_a_moved_copy_repeats_the_move() {
    let mut s = session();
    let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 50, 50]})).unwrap();
    let a = r["id"].as_u64().unwrap();
    let boxed = |s: &mut Session, id: u64| {
        let v = s.execute("shape.inspect", &json!({"id": id})).unwrap();
        (v["box"]["x"].as_f64().unwrap(), v["box"]["y"].as_f64().unwrap())
    };
    let dup = |s: &mut Session| s.execute("edit.duplicate", &json!({})).unwrap()["ids"][0].as_u64().unwrap();
    // The first copy gets the usual offset.
    let b = dup(&mut s);
    let off = s.prefs.duplicate_offset;
    assert_eq!(boxed(&mut s, b), (100.0 + off, 100.0 + off));
    // Moved, then duplicated again: the next copy repeats the move from the original (#966).
    s.execute("shape.move", &json!({"dx": 80.0 - off, "dy": -off})).unwrap();
    assert_eq!(boxed(&mut s, b), (180.0, 100.0));
    let c = dup(&mut s);
    assert_eq!(boxed(&mut s, c), (260.0, 100.0));
    let d = dup(&mut s);
    assert_eq!(boxed(&mut s, d), (340.0, 100.0));
    // Duplicating something else goes back to the usual offset.
    s.doc_mut().unwrap().selection.shapes = vec![ShapeId(a as u32)];
    let e = dup(&mut s);
    assert_eq!(boxed(&mut s, e), (100.0 + off, 100.0 + off));
    // Undone, its id can go to a new shape: duplicating that one uses the usual offset too.
    s.execute("edit.undo", &json!({})).unwrap();
    let r = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [400, 300, 40, 40]})).unwrap();
    let g = r["id"].as_u64().unwrap();
    assert_eq!(g, e, "the new shape reuses the undone copy's id");
    let h = dup(&mut s);
    assert_eq!(boxed(&mut s, h), (400.0 + off, 300.0 + off));
    // After Undo removed that copy, duplicating the last moved copy gets the usual offset too.
    s.execute("edit.undo", &json!({})).unwrap();
    s.doc_mut().unwrap().selection.shapes = vec![ShapeId(d as u32)];
    let f = dup(&mut s);
    assert_eq!(boxed(&mut s, f), (340.0 + off, 100.0 + off));
    assert_eq!(s.doc().unwrap().doc.validate(), Vec::<String>::new());
}
