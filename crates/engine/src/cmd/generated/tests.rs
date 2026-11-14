use deckcraft_model::{CheckState, Generated, Geom, HarveyBall, ShapeId, ShapeKind};
use serde_json::{Value, json};

use crate::{Session, find_command};

fn deck() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s
}

fn id(v: &Value) -> u32 {
    v["id"].as_u64().unwrap() as u32
}

fn shape(s: &Session, id: u32) -> deckcraft_model::Shape {
    s.doc().unwrap().shape(ShapeId(id)).cloned().unwrap()
}

fn kids(s: &Session, id: u32) -> Vec<deckcraft_model::Shape> {
    shape(s, id).children().to_vec()
}

fn ball(s: &Session, id: u32) -> HarveyBall {
    match shape(s, id).generated {
        Some(Generated::HarveyBall(b)) => b,
        other => panic!("not a ball: {other:?}"),
    }
}

fn undo_len(s: &Session) -> usize {
    s.doc().unwrap().history.undo.len()
}

#[test]
fn the_five_commands_are_registered() {
    for c in ["insert.harveyBall", "harveyBall.set", "insert.checkbox", "checkbox.set", "checkbox.toggle"] {
        let spec = find_command(c).unwrap_or_else(|| panic!("{c} is not registered"));
        assert!(spec.journal && spec.undoable, "{c}");
    }
}

/// C1 acceptance 2: the wedge's end angle is exact for thirds, eighths, tenths and hundredths; an
/// empty ball has no wedge and a full one a disc.
#[test]
fn wedge_angles_are_exact() {
    let mut s = deck();
    for (value, of) in [(0u64, 4u64), (1, 4), (2, 4), (3, 4), (4, 4), (1, 3), (2, 3), (5, 8), (7, 10), (1, 100)] {
        let g = id(&s.execute("insert.harveyBall", &json!({"value": value, "of": of})).unwrap());
        let k = kids(&s, g);
        let names: Vec<_> = k.iter().map(|c| c.geom.preset_name().unwrap_or("custom")).collect();
        match (value, of) {
            (0, _) => assert_eq!(names, ["ellipse", "ellipse"], "{value}/{of}: background and ring"),
            (v, o) if v == o => assert_eq!(names, ["ellipse", "ellipse", "ellipse"], "{value}/{of}: a full disc"),
            _ => {
                assert_eq!(names, ["ellipse", "pie", "ellipse"], "{value}/{of}");
                let Geom::Preset { adj, .. } = &k[1].geom else { panic!() };
                assert_eq!(adj[0], 16_200_000.0, "{value}/{of}: from 12 o'clock");
                assert_eq!(adj[1], ((16_200_000 + 21_600_000 * value / of) % 21_600_000) as f64, "{value}/{of}");
                assert_eq!(adj[1].fract(), 0.0);
            }
        }
    }
    assert_eq!(super::wedge_end(1, 3), Some(16_200_000 + 7_200_000 - 21_600_000));
    assert_eq!(super::wedge_end(1, 7), Some(16_200_000 + 3_085_714), "21600000 / 7 rounded to the nearest");
    assert_eq!(super::wedge_end(1, 0), None);
}

#[test]
fn a_ball_is_drawn_in_the_largest_centred_square() {
    let mut s = deck();
    let g = id(&s.execute("insert.harveyBall", &json!({"value": 1, "rect": [100, 50, 80, 40], "background": "accent2", "outline": null})).unwrap());
    let k = kids(&s, g);
    assert_eq!(k.len(), 2, "no ring without an outline");
    for c in &k {
        let x = c.xfrm.unwrap();
        assert_eq!((x.x, x.y, x.w, x.h), (120.0, 50.0, 40.0, 40.0));
    }
    assert!(matches!(&k[0].fill, Some(deckcraft_model::Fill::Solid { .. })), "the background is filled");
    // Default size and place: a 14 pt square in the slide's middle.
    let g = id(&s.execute("insert.harveyBall", &json!({})).unwrap());
    let x = shape(&s, g).xfrm.unwrap();
    let size = s.doc().unwrap().doc.slide_size;
    assert_eq!((x.w, x.h), (14.0, 14.0));
    assert!(((x.x + 7.0) - size.width / 2.0).abs() < 1e-9 && ((x.y + 7.0) - size.height / 2.0).abs() < 1e-9);
    assert_eq!(ball(&s, g), HarveyBall::default());
    assert!(shape(&s, g).name.starts_with("Harvey Ball "), "named like other inserted shapes: {}", shape(&s, g).name);
    let g = id(&s.execute("insert.harveyBall", &json!({"size": 40})).unwrap());
    assert_eq!(shape(&s, g).xfrm.map(|x| (x.w, x.h)), Some((40.0, 40.0)));
}

#[test]
fn checkbox_shapes_follow_its_state() {
    let mut s = deck();
    let shapes = |s: &mut Session, p: Value| {
        let g = id(&s.execute("insert.checkbox", &p).unwrap());
        kids(s, g)
    };
    let k = shapes(&mut s, json!({"size": 28}));
    assert_eq!(k.len(), 2);
    assert_eq!(k[0].geom.preset_name(), Some("rect"));
    assert_eq!(k[0].line.as_ref().and_then(|l| l.width), Some(2.0), "box line: size / 14");
    let Geom::Custom { paths } = &k[1].geom else { panic!("the tick is a custom path") };
    assert_eq!(paths[0].d, "M 20 55 L 42 75 L 80 28");
    assert_eq!(paths[0].fill, deckcraft_geom::preset::FillMode::None);
    let line = k[1].line.as_ref().unwrap();
    assert!((line.width.unwrap() - 28.0 / 9.0).abs() < 1e-12);
    assert_eq!((line.cap, line.join), (Some(deckcraft_model::style::LineCap::Round), Some(deckcraft_model::style::LineJoin::Round)));
    assert_eq!(shapes(&mut s, json!({"state": "unchecked"})).len(), 1, "the box alone");
    let k = shapes(&mut s, json!({"state": "crossed"}));
    let Geom::Custom { paths } = &k[1].geom else { panic!() };
    assert_eq!(paths[0].d, "M 25 25 L 75 75 M 75 25 L 25 75");
    let k = shapes(&mut s, json!({"boxed": false}));
    assert!(k[0].line.as_ref().is_some_and(|l| matches!(l.fill, Some(deckcraft_model::Fill::None))), "an unboxed checkbox keeps an invisible box");
    let k = shapes(&mut s, json!({"size": 7}));
    assert_eq!(k[0].line.as_ref().and_then(|l| l.width), Some(0.75), "box line at least 0.75 pt");
}

/// C1 acceptance 5: every rejected value is an error naming the command, nothing is added or
/// changed, and nothing panics.
#[test]
fn every_rejection_names_the_command() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({})).unwrap());
    let c = id(&s.execute("insert.checkbox", &json!({})).unwrap());
    let cases: Vec<(&str, Value)> = vec![
        ("insert.harveyBall", json!({"of": 0})),
        ("insert.harveyBall", json!({"of": 101})),
        ("insert.harveyBall", json!({"value": 5, "of": 4})),
        ("insert.harveyBall", json!({"value": 1.5})),
        ("insert.harveyBall", json!({"of": "4"})),
        ("insert.harveyBall", json!({"value": -1})),
        ("insert.harveyBall", json!({"size": 3})),
        ("insert.harveyBall", json!({"size": 1e308})),
        ("insert.harveyBall", json!({"outlineWidth": 21})),
        ("insert.harveyBall", json!({"outlineWidth": "thick"})),
        ("insert.harveyBall", json!({"fill": "notAColour"})),
        ("insert.harveyBall", json!({"background": 5})),
        ("insert.harveyBall", json!({"rect": [0, 0, 0, 10]})),
        ("insert.harveyBall", json!({"rect": [0, 0, -10, 10]})),
        ("insert.harveyBall", json!({"rect": "bad"})),
        ("insert.checkbox", json!({"state": "maybe"})),
        ("insert.checkbox", json!({"color": {"scheme": "nope"}})),
        ("insert.checkbox", json!({"size": 401})),
        ("insert.checkbox", json!({"boxed": "yes"})),
        ("insert.checkbox", json!({"rect": [0, 0, 10, 0]})),
        ("harveyBall.set", json!({"id": b, "of": 0})),
        ("harveyBall.set", json!({"id": b, "value": 7})),
        ("harveyBall.set", json!({"id": c})),
        ("checkbox.set", json!({"id": b})),
        ("checkbox.set", json!({"id": c, "state": 1})),
        ("checkbox.toggle", json!({"id": b})),
    ];
    let before = s.doc().unwrap().doc.clone();
    let undo = undo_len(&s);
    for (cmd, p) in cases {
        let e = s.execute(cmd, &p).expect_err(&format!("{cmd} {p} should fail"));
        assert!(e.to_string().contains(cmd), "{cmd} {p}: {e}");
    }
    assert_eq!(*s.doc().unwrap().doc, *before, "nothing changed");
    assert_eq!(undo_len(&s), undo);
    // The wrong kind, by selection: greyed out, saying what to select.
    s.execute("edit.select", &json!({"ids": [c]})).unwrap();
    let e = s.execute("harveyBall.set", &json!({"value": 2})).unwrap_err().to_string();
    assert!(e.contains("harveyBall.set") && e.contains("select a Harvey ball"), "{e}");
}

/// C1 acceptance 6, and partial updates checked as a whole.
#[test]
fn set_and_toggle_are_one_undo_step_each() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 3})).unwrap());
    let c = id(&s.execute("insert.checkbox", &json!({})).unwrap());
    let n = undo_len(&s);
    s.execute("harveyBall.set", &json!({"id": b, "value": 2, "of": 8, "fill": "accent2", "outlineWidth": 2})).unwrap();
    assert_eq!(undo_len(&s), n + 1);
    let got = ball(&s, b);
    assert_eq!((got.value, got.of, got.outline_width), (2, 8, 2.0));
    s.execute("checkbox.set", &json!({"id": c, "state": "crossed", "color": "#C00000"})).unwrap();
    assert_eq!(undo_len(&s), n + 2);
    let r = s.execute("checkbox.toggle", &json!({"id": c})).unwrap();
    assert_eq!((id(&r), r["state"].as_str()), (c, Some("checked")), "crossed → checked");
    assert_eq!(undo_len(&s), n + 3);
    assert_eq!(s.execute("checkbox.toggle", &json!({"id": c})).unwrap()["state"], "unchecked");
    assert_eq!(kids(&s, c).len(), 1);
    // `{of: 2}` on a ball at 2 of 8 would leave value > of: refused, nothing changes.
    s.execute("harveyBall.set", &json!({"id": b, "value": 3})).unwrap();
    let before = ball(&s, b);
    assert!(s.execute("harveyBall.set", &json!({"id": b, "of": 2})).is_err());
    assert_eq!(ball(&s, b), before);
    s.execute("harveyBall.set", &json!({"id": b, "of": 2, "value": 2})).unwrap();
    assert_eq!((ball(&s, b).value, ball(&s, b).of), (2, 2));
    // Undo puts the ball's description and its shapes back together.
    let shapes_before = kids(&s, b);
    s.execute("harveyBall.set", &json!({"id": b, "value": 0})).unwrap();
    assert_eq!(kids(&s, b).len(), 2);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(kids(&s, b), shapes_before);
}

#[test]
fn a_part_or_the_selection_finds_its_element() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 1})).unwrap());
    let wedge = kids(&s, b)[1].id.0;
    // By a part's id, whatever is selected.
    let other = id(&s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap());
    assert_eq!(id(&s.execute("harveyBall.set", &json!({"id": wedge, "value": 2})).unwrap()), b);
    assert_eq!(ball(&s, b).value, 2);
    assert_eq!(s.doc().unwrap().selection.shapes, [ShapeId(other)], "the selection stays");
    // By a selected part.
    s.execute("edit.select", &json!({"ids": [wedge]})).unwrap();
    s.execute("harveyBall.set", &json!({"value": 3})).unwrap();
    assert_eq!(ball(&s, b).value, 3);
    // A plain group holding a ball isn't one.
    s.execute("edit.select", &json!({"ids": [b, other]})).unwrap();
    let outer = id(&s.execute("arrange.group", &json!({})).unwrap());
    assert!(s.execute("harveyBall.set", &json!({"id": outer, "value": 1})).is_err());
    assert!(s.execute("harveyBall.set", &json!({"id": b, "value": 1})).is_ok(), "the ball inside still is");
    assert_eq!(ball(&s, b).value, 1);
}

#[test]
fn several_selected_elements_change_together() {
    let mut s = deck();
    let a = id(&s.execute("insert.harveyBall", &json!({"value": 1})).unwrap());
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 3})).unwrap());
    let c = id(&s.execute("insert.checkbox", &json!({})).unwrap());
    s.execute("edit.select", &json!({"ids": [a, b, c]})).unwrap();
    let n = undo_len(&s);
    let r = s.execute("harveyBall.set", &json!({"fill": "accent2"})).unwrap();
    assert_eq!(r["ids"], json!([a, b]));
    assert_eq!(undo_len(&s), n + 1, "one undo step for both");
    let accent2 = deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Accent2);
    assert_eq!((ball(&s, a).fill, ball(&s, b).fill), (accent2.clone(), accent2));
    assert!(matches!(shape(&s, c).generated, Some(Generated::Checkbox(_))), "the checkbox is left alone");
    // One fails, none change.
    assert!(s.execute("harveyBall.set", &json!({"of": 2})).is_err());
    assert_eq!((ball(&s, a).of, ball(&s, b).of), (4, 4));
    s.execute("edit.select", &json!({"ids": [c]})).unwrap();
    assert!(s.execute("harveyBall.set", &json!({"fill": "accent3"})).is_err(), "none qualify");
}

#[test]
fn resizing_keeps_a_ball_round() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 1, "rect": [100, 100, 50, 50]})).unwrap());
    let ids_before: Vec<_> = kids(&s, b).iter().map(|c| c.id).collect();
    let n = undo_len(&s);
    s.execute("shape.sizePosition", &json!({"id": b, "w": 120, "h": 40})).unwrap();
    assert_eq!(undo_len(&s), n + 1, "the redraw is part of the resize");
    let g = shape(&s, b);
    let (x, ShapeKind::Group { child, .. }) = (g.xfrm.unwrap(), &g.kind) else { panic!() };
    assert_eq!((child.w, child.h), (x.w, x.h), "drawn 1:1");
    for c in g.children() {
        let cx = c.xfrm.unwrap();
        assert_eq!((cx.w, cx.h), (40.0, 40.0), "a circle in the largest square");
        assert!(((cx.x + 20.0) - (child.x + child.w / 2.0)).abs() < 1e-9, "centred");
    }
    assert_eq!(g.children().iter().map(|c| c.id).collect::<Vec<_>>(), ids_before, "the parts keep their ids");
    // A move or rotation redraws nothing.
    s.execute("shape.move", &json!({"id": b, "dx": 10, "dy": 0})).unwrap();
    s.execute("shape.rotate", &json!({"id": b, "deg": 30})).unwrap();
    assert_eq!(shape(&s, b).children(), g.children());
}

#[test]
fn parts_move_with_their_element_only() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 1})).unwrap());
    let wedge = kids(&s, b)[1].id.0;
    let drawn_fill = kids(&s, b)[1].fill.clone();
    let before = s.doc().unwrap().doc.clone();
    for (cmd, p) in [
        ("shape.move", json!({"id": wedge, "dx": 5, "dy": 0})),
        ("shape.sizePosition", json!({"id": wedge, "w": 3})),
        ("edit.delete", json!({"ids": [wedge]})),
        ("arrange.bringToFront", json!({"id": wedge})),
        ("shape.change", json!({"ids": [wedge], "preset": "rect"})),
        ("shape.adjust", json!({"ids": [wedge], "index": 1, "value": 10_800_000})),
    ] {
        let r = s.execute(cmd, &p);
        assert!(r.is_err(), "{cmd}: {r:?}");
        assert_eq!(*s.doc().unwrap().doc, *before, "{cmd} changed nothing");
    }
    // Formatting a part is allowed; the next redraw replaces it.
    s.execute("shape.fill", &json!({"id": wedge, "color": "#FF0000"})).unwrap();
    assert_ne!(kids(&s, b)[1].fill, drawn_fill);
    s.execute("harveyBall.set", &json!({"id": b})).unwrap();
    assert_eq!(kids(&s, b)[1].fill, drawn_fill);
    // The whole element moves and deletes as usual.
    s.execute("shape.move", &json!({"id": b, "dx": 5, "dy": 5})).unwrap();
    s.execute("edit.delete", &json!({"ids": [b]})).unwrap();
    assert!(s.doc().unwrap().shape(ShapeId(b)).is_none());
}

#[test]
fn copies_keep_the_description_and_ungrouping_drops_it() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 3})).unwrap());
    s.execute("edit.select", &json!({"ids": [b]})).unwrap();
    s.execute("edit.duplicate", &json!({})).unwrap();
    let copy = s.doc().unwrap().selection.shapes[0];
    assert_ne!(copy.0, b);
    assert_eq!(shape(&s, copy.0).generated, shape(&s, b).generated);
    s.execute("harveyBall.set", &json!({"id": copy.0, "value": 1})).unwrap();
    assert_eq!((ball(&s, copy.0).value, ball(&s, b).value), (1, 3), "the copy is its own");
    s.execute("edit.copy", &json!({})).unwrap();
    s.execute("edit.paste", &json!({})).unwrap();
    let pasted = s.doc().unwrap().selection.shapes[0];
    assert_eq!(ball(&s, pasted.0).value, 1);
    s.execute("arrange.ungroup", &json!({"id": b})).unwrap();
    let st = s.doc().unwrap();
    assert!(st.shape(ShapeId(b)).is_none());
    let mut generated = 0;
    deckcraft_model::walk(&st.current_slide().unwrap().shapes, &mut |x, _| generated += usize::from(x.generated.is_some()));
    assert_eq!(generated, 2, "the ungrouped ball's parts are plain shapes");
}

#[test]
fn space_toggles_a_selected_checkbox() {
    let mut s = deck();
    let c = id(&s.execute("insert.checkbox", &json!({})).unwrap());
    let n = undo_len(&s);
    let r = s.key("Space", crate::Mods::default()).unwrap();
    assert_eq!(r["command"], "checkbox.toggle");
    let state = |s: &Session| match shape(s, c).generated {
        Some(Generated::Checkbox(x)) => x.state,
        _ => panic!(),
    };
    assert_eq!(state(&s), CheckState::Unchecked);
    assert_eq!(undo_len(&s), n + 1);
    // The space the key also types neither edits the group nor errs.
    assert_eq!(s.type_text(" ").unwrap()["handled"], true);
    assert_eq!(state(&s), CheckState::Unchecked);
    assert!(s.doc().unwrap().selection.text.is_none());
    // A key sent as the character itself (the control channel, MCP) toggles too.
    assert_eq!(s.key(" ", crate::Mods::default()).unwrap()["command"], "checkbox.toggle");
    assert_eq!(state(&s), CheckState::Checked);
    // Elsewhere Space is what it was.
    let r = id(&s.execute("shape.insert", &json!({"preset": "rect", "rect": [10, 10, 50, 50]})).unwrap());
    s.execute("edit.select", &json!({"ids": [r]})).unwrap();
    assert_eq!(s.key("Space", crate::Mods::default()).unwrap()["handled"], false);
}

/// C1 acceptance 1 without the CLI process: the element survives a `.pptx` save and open, and
/// `slide.inspect` (which `deckcraft-cli run --print` shows) names what it was generated from.
#[test]
fn inspect_shows_what_a_group_was_generated_from() {
    let mut s = deck();
    s.execute("insert.harveyBall", &json!({"value": 3, "of": 4})).unwrap();
    let data = s.execute("file.saveBytes", &json!({"format": "pptx"})).unwrap()["data"].as_str().unwrap().to_string();
    s.execute("file.openBytes", &json!({"name": "t.pptx", "data": data})).unwrap();
    let n = s.doc().unwrap().doc.slides.len();
    let shapes: Vec<Value> =
        (0..n).flat_map(|i| s.execute("slide.inspect", &json!({"index": i})).unwrap()["shapes"].as_array().cloned().unwrap_or_default()).collect();
    let g = shapes.iter().find(|x| x["kind"] == "group").unwrap_or_else(|| panic!("{shapes:?}"));
    assert_eq!(
        (g["generated"]["kind"].as_str(), g["generated"]["value"].as_u64(), g["generated"]["of"].as_u64()),
        (Some("harveyBall"), Some(3), Some(4))
    );
    let presets: Vec<_> = g["children"].as_array().unwrap().iter().map(|c| c["preset"].as_str().unwrap_or_default()).collect();
    assert_eq!(presets, ["ellipse", "pie", "ellipse"]);
}

/// A description no command would write (a hostile `.deckcraft`) never panics: a resize leaves the
/// drawing as it is and `*.set` explains what is wrong.
#[test]
fn an_undrawable_description_is_refused_not_drawn() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({})).unwrap());
    {
        let st = s.doc_mut().unwrap();
        let doc = std::sync::Arc::make_mut(&mut st.doc);
        let sel = st.selection.clone();
        let g = deckcraft_model::find_shape_mut(crate::shapes_mut(doc, &sel).unwrap(), ShapeId(b)).unwrap();
        g.generated = Some(Generated::HarveyBall(HarveyBall { of: 0, value: 9, outline_width: f64::NAN, ..Default::default() }));
    }
    let drawn = kids(&s, b);
    s.execute("shape.sizePosition", &json!({"id": b, "w": 80, "h": 20})).unwrap();
    assert_eq!(kids(&s, b), drawn);
    let e = s.execute("harveyBall.set", &json!({"id": b, "fill": "accent1"})).unwrap_err().to_string();
    assert!(e.contains("`of` must be 1 to 100"), "{e}");
    s.execute("harveyBall.set", &json!({"id": b, "of": 10, "value": 9, "outlineWidth": 1})).unwrap();
    assert_eq!(ball(&s, b).of, 10);
}

/// A deck whose ids are used up refuses a new element instead of reusing ids (#407).
#[test]
fn an_exhausted_id_space_refuses_new_elements() {
    let mut s = deck();
    std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().doc).next_id = u32::MAX - 1;
    let before = s.doc().unwrap().doc.clone();
    assert!(s.execute("insert.harveyBall", &json!({})).is_err());
    assert!(s.execute("insert.checkbox", &json!({})).is_err());
    assert_eq!(*s.doc().unwrap().doc, *before);
}

/// Shape Fill and Shape Outline on a selected element change its description, so the colour
/// survives the redraw a resize makes; other selected shapes are formatted as usual, all in one
/// undo step.
#[test]
fn shape_fill_and_outline_recolour_an_element_through_a_resize() {
    use deckcraft_model::Fill;
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 1, "rect": [100, 100, 50, 50]})).unwrap());
    let c = id(&s.execute("insert.checkbox", &json!({"rect": [200, 100, 40, 40]})).unwrap());
    let r = id(&s.execute("shape.insert", &json!({"preset": "rect", "rect": [300, 100, 50, 50]})).unwrap());
    s.execute("edit.select", &json!({"ids": [b, c, r]})).unwrap();
    let n = undo_len(&s);
    s.execute("shape.fill", &json!({"color": "#FF0000"})).unwrap();
    assert_eq!(undo_len(&s), n + 1, "one undo step for all three");
    let red = crate::cmd::color_value(&json!("#FF0000")).unwrap();
    let accent2 = crate::cmd::color_value(&json!("accent2")).unwrap();
    let check = |s: &Session| match shape(s, c).generated {
        Some(Generated::Checkbox(x)) => x,
        _ => panic!(),
    };
    assert_eq!(ball(&s, b).fill, red);
    assert_eq!(check(&s).color, red);
    assert_eq!(shape(&s, r).fill, Some(Fill::solid(red.clone())), "the rectangle is filled as usual");
    s.execute("shape.line", &json!({"ids": [b], "color": "accent2", "width": 3})).unwrap();
    assert_eq!((ball(&s, b).outline, ball(&s, b).outline_width), (Some(accent2.clone()), 3.0));
    s.execute("shape.line", &json!({"ids": [c], "color": "accent2"})).unwrap();
    assert_eq!(check(&s).color, accent2);
    // Resized, both are redrawn with their colours.
    s.execute("shape.sizePosition", &json!({"id": b, "w": 120, "h": 60})).unwrap();
    s.execute("shape.sizePosition", &json!({"id": c, "w": 90, "h": 40})).unwrap();
    let k = kids(&s, b);
    assert_eq!(k[1].fill, Some(Fill::solid(red.clone())), "the wedge");
    let ring = k[2].line.as_ref().unwrap();
    assert_eq!((ring.fill.clone(), ring.width), (Some(Fill::solid(accent2.clone())), Some(3.0)));
    assert!(kids(&s, c).iter().all(|x| x.line.as_ref().and_then(|l| l.fill.clone()) == Some(Fill::solid(accent2.clone()))), "box and tick");
    // No outline takes the ring off; a weight puts a Text 1 ring back.
    s.execute("shape.line", &json!({"ids": [b], "none": true})).unwrap();
    assert_eq!((ball(&s, b).outline, kids(&s, b).len()), (None, 2));
    s.execute("shape.line", &json!({"ids": [b], "width": 2})).unwrap();
    assert_eq!((ball(&s, b).outline, ball(&s, b).outline_width), (crate::cmd::color_value(&json!("tx1")), 2.0));
    s.execute("shape.fill", &json!({"ids": [b], "reset": true})).unwrap();
    assert_eq!(ball(&s, b).fill, HarveyBall::default().fill);
    // What a description can't hold is refused, naming the command, and changes nothing.
    let before = s.doc().unwrap().doc.clone();
    for (cmd, p) in [
        ("shape.fill", json!({"ids": [b], "gradient": {"stops": [[0, "#fff"], [1, "#000"]]}})),
        ("shape.fill", json!({"ids": [c], "none": true})),
        ("shape.fill", json!({"ids": [b, r], "color": "notAColour"})),
        ("shape.line", json!({"ids": [b], "dash": "dash"})),
        ("shape.line", json!({"ids": [b], "width": 25})),
        ("shape.line", json!({"ids": [c], "width": 2})),
        ("shape.line", json!({"ids": [c], "none": true})),
    ] {
        let e = s.execute(cmd, &p).expect_err(&format!("{cmd} {p}")).to_string();
        assert!(e.contains(cmd), "{cmd} {p}: {e}");
        assert_eq!(*s.doc().unwrap().doc, *before, "{cmd} {p} changed nothing");
    }
}

/// Flip is refused for a Harvey ball or checkbox, as for a Diagram graphic, also inside a group;
/// a resize dragged past the opposite edge keeps it the right way round.
#[test]
fn generated_elements_are_not_flipped() {
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 1, "rect": [100, 100, 50, 50]})).unwrap());
    let c = id(&s.execute("insert.checkbox", &json!({"rect": [200, 100, 40, 40]})).unwrap());
    let before = s.doc().unwrap().doc.clone();
    for (e, axis) in [(b, "horizontal"), (b, "vertical"), (c, "horizontal"), (c, "vertical")] {
        let err = s.execute("shape.flip", &json!({"id": e, "axis": axis})).unwrap_err().to_string();
        assert!(err.contains("shape.flip") && err.contains("Harvey ball or checkbox"), "{err}");
    }
    assert_eq!(*s.doc().unwrap().doc, *before);
    let r = id(&s.execute("shape.insert", &json!({"preset": "rect", "rect": [300, 100, 50, 50]})).unwrap());
    s.execute("edit.select", &json!({"ids": [b, r]})).unwrap();
    let outer = id(&s.execute("arrange.group", &json!({})).unwrap());
    assert!(s.execute("shape.flip", &json!({"id": outer})).is_err(), "a group holding a ball");
    // A handle drag past the opposite edge flips what it resizes (`tools::hit::resize_box`).
    let old = s.doc().unwrap().doc.clone();
    let mut new = (*old).clone();
    let slide = s.doc().unwrap().selection.slide;
    let sh = deckcraft_model::find_shape_mut(&mut std::sync::Arc::make_mut(&mut new.slides[slide]).shapes, ShapeId(c)).unwrap();
    let x = sh.xfrm.as_mut().unwrap();
    (x.flip_h, x.flip_v, x.w) = (true, true, 60.0);
    super::keep_whole(&old, &mut new).unwrap();
    let sh = deckcraft_model::find_shape(&new.slides[slide].shapes, ShapeId(c)).unwrap();
    assert_eq!(sh.xfrm.map(|x| (x.flip_h, x.flip_v, x.w)), Some((false, false, 60.0)), "resized, not mirrored");
}

/// Formatting an ordinary group passes its colours on to the Harvey balls and checkboxes inside, so
/// a redraw keeps them; a format their description can't hold leaves them as they are and formats
/// the rest of the group.
#[test]
fn formatting_a_group_passes_colours_on_to_the_elements_inside() {
    use deckcraft_model::Fill;
    let mut s = deck();
    let b = id(&s.execute("insert.harveyBall", &json!({"value": 1, "rect": [100, 100, 50, 50]})).unwrap());
    let c = id(&s.execute("insert.checkbox", &json!({"rect": [200, 100, 40, 40]})).unwrap());
    let r = id(&s.execute("shape.insert", &json!({"preset": "rect", "rect": [300, 100, 50, 50]})).unwrap());
    s.execute("edit.select", &json!({"ids": [b, c, r]})).unwrap();
    let outer = id(&s.execute("arrange.group", &json!({})).unwrap());
    let red = crate::cmd::color_value(&json!("#FF0000")).unwrap();
    let accent2 = crate::cmd::color_value(&json!("accent2")).unwrap();
    let check = |s: &Session| match shape(s, c).generated {
        Some(Generated::Checkbox(x)) => x,
        _ => panic!(),
    };
    let n = undo_len(&s);
    s.execute("shape.fill", &json!({"id": outer, "color": "#FF0000"})).unwrap();
    assert_eq!(undo_len(&s), n + 1, "one undo step");
    assert_eq!((ball(&s, b).fill, check(&s).color), (red.clone(), red.clone()));
    assert_eq!(shape(&s, r).fill, Some(Fill::solid(red.clone())), "the rest of the group as usual");
    assert_eq!(kids(&s, b)[0].fill, Some(Fill::None), "the ball's background isn't painted over");
    // As the ribbon sends them: an outline colour, then a weight (which a checkbox can't hold).
    s.execute("shape.line", &json!({"id": outer, "color": "accent2"})).unwrap();
    assert_eq!((ball(&s, b).outline, check(&s).color), (Some(accent2.clone()), accent2.clone()));
    s.execute("shape.line", &json!({"id": outer, "width": 2})).unwrap();
    assert_eq!((ball(&s, b).outline_width, check(&s).color), (2.0, accent2.clone()));
    // A redraw keeps them.
    s.execute("harveyBall.set", &json!({"id": b, "value": 3})).unwrap();
    assert_eq!(kids(&s, b)[1].fill, Some(Fill::solid(red.clone())));
    // A gradient or dashes, which a description can't hold: the elements stay as they are.
    let (ball_before, check_before) = (shape(&s, b), shape(&s, c));
    s.execute("shape.fill", &json!({"id": outer, "gradient": {"stops": [[0, "#ffffff"], [1, "#000000"]]}})).unwrap();
    s.execute("shape.line", &json!({"id": outer, "dash": "dash"})).unwrap();
    assert_eq!((shape(&s, b), shape(&s, c)), (ball_before, check_before));
    assert!(matches!(shape(&s, r).fill, Some(Fill::Gradient(_))), "the rectangle takes the gradient");
}
