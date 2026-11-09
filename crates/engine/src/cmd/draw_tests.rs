//! Draw ▸ Stencils ▸ Ruler: the view state, its commands, and how ink and dragged objects snap.

use serde_json::json;

use crate::ruler::{HALF_WIDTH, LENGTH, MAX_OFFSET, SNAP};
use crate::tools::{Mods, PointerEvent, PointerKind, ToolKind};
use crate::{Session, model};

fn blank() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.prefs.snap_to_grid = false;
    s
}

fn ruler(s: &mut Session) -> serde_json::Value {
    s.execute("draw.rulerState", &json!({})).unwrap()
}

#[test]
fn the_ribbon_button_toggles_and_the_first_show_centres_the_page() {
    let mut s = blank();
    let r = s.execute("draw.ruler", &json!({})).unwrap();
    assert_eq!(r["visible"], true, "{r}");
    assert_eq!((r["x"].as_f64(), r["y"].as_f64()), (Some(480.0), Some(270.0)), "the page centre: {r}");
    assert_eq!(r["angle"].as_f64(), Some(45.0), "PowerPoint's first angle");
    assert_eq!(r["length"].as_f64(), Some(LENGTH));
    assert_eq!(r["snap"].as_f64(), Some(SNAP));
    // Hiding and showing again keeps where it was put.
    s.execute("draw.ruler", &json!({"x": 100, "y": 120, "angle": 10})).unwrap();
    s.execute("draw.ruler", &json!({"on": false})).unwrap();
    assert_eq!(ruler(&mut s)["visible"], false);
    let r = s.execute("draw.ruler", &json!({"on": true})).unwrap();
    assert_eq!(r["visible"], true);
    assert_eq!((r["x"].as_f64(), r["y"].as_f64(), r["angle"].as_f64()), (Some(100.0), Some(120.0), Some(10.0)), "{r}");
    // `on` alone is not a toggle.
    let r = s.execute("draw.ruler", &json!({"on": true})).unwrap();
    assert_eq!(r["visible"], true);
    // Without a page there is nothing to centre on, and nothing crashes.
    let mut empty = Session::new();
    let r = empty.execute("draw.ruler", &json!({})).unwrap();
    assert_eq!((r["visible"].clone(), r["x"].clone(), r["y"].clone()), (json!(true), json!(0.0), json!(0.0)));
}

#[test]
fn angle_wraps_and_the_position_is_bounded() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true})).unwrap();
    for (given, want) in [(-30.0, 330.0), (360.0, 0.0), (725.5, 5.5), (-0.0, 0.0)] {
        let r = s.execute("draw.ruler", &json!({"angle": given})).unwrap();
        assert_eq!(r["angle"].as_f64(), Some(want), "{given}");
    }
    let r = s.execute("draw.ruler", &json!({"x": 1e300, "y": -1e300})).unwrap();
    assert_eq!((r["x"].as_f64(), r["y"].as_f64()), (Some(MAX_OFFSET), Some(-MAX_OFFSET)));
    // Hostile parameters are told, not guessed.
    assert!(s.execute("draw.ruler", &json!({"angle": "north"})).is_err());
    assert!(s.execute("draw.ruler", &json!({"x": null, "y": "3"})).is_err());
    assert!(s.execute("draw.ruler", &json!({"on": "yes"})).is_err());
    // The query is always available, even with no presentation.
    assert!(Session::new().execute("draw.rulerState", &json!({})).is_ok());
}

#[test]
fn shift_f6_moves_and_turns_the_ruler() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 200, "angle": 0})).unwrap();
    // Without manipulation mode, Shift+F6 with no visible ruler does nothing.
    let hidden = s.execute("draw.ruler", &json!({"on": false})).unwrap();
    assert_eq!(hidden["visible"], false);
    let r = s.key("F6", Mods { shift: true, ..Mods::default() }).unwrap();
    assert_eq!(r["handled"], false);
    s.execute("draw.ruler", &json!({"on": true})).unwrap();
    let r = s.key("F6", Mods { shift: true, ..Mods::default() }).unwrap();
    assert_eq!(r["handled"], true);
    assert_eq!(ruler(&mut s)["manipulate"], true);
    s.key("Right", Mods::default()).unwrap();
    s.key("Down", Mods { shift: true, ..Mods::default() }).unwrap();
    let st = ruler(&mut s);
    assert_eq!((st["x"].as_f64(), st["y"].as_f64()), (Some(306.0), Some(206.0)), "{st}");
    s.key("Right", Mods { alt: true, ..Mods::default() }).unwrap();
    assert_eq!(ruler(&mut s)["angle"].as_f64(), Some(15.0));
    s.key("Right", Mods { alt: true, cmd: true, ..Mods::default() }).unwrap();
    assert_eq!(ruler(&mut s)["angle"].as_f64(), Some(16.0));
    s.key("Left", Mods { alt: true, ..Mods::default() }).unwrap();
    assert_eq!(ruler(&mut s)["angle"].as_f64(), Some(1.0), "wraps below zero");
    // Escape leaves manipulation mode; the arrow keys nudge shapes again.
    s.key("Escape", Mods::default()).unwrap();
    assert_eq!(ruler(&mut s)["manipulate"], false);
}

fn ink_strokes(s: &Session) -> Vec<Vec<(f64, f64)>> {
    let Some(sh) = s.doc().ok().and_then(|d| d.shapes().iter().rev().find(|x| matches!(x.kind, model::ShapeKind::Ink { .. }))) else {
        return vec![];
    };
    let model::ShapeKind::Ink { strokes } = &sh.kind else { return vec![] };
    strokes.iter().map(|st| st.points.iter().map(|p| (p.0, p.1)).collect()).collect()
}

fn pen(s: &mut Session) {
    s.set_tool(ToolKind::Ink { mode: "pen".into(), color: model::Rgba::rgb(10, 10, 10), width: 3.0 });
}

fn ev(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
    PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None }
}

#[test]
fn ink_near_the_ruler_is_drawn_straight_along_it() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 300, "angle": 0})).unwrap();
    pen(&mut s);
    // Drawn along the body a little below the lower edge, wobbling within the snap threshold.
    let edge = 300.0 + HALF_WIDTH;
    s.pointer(ev(PointerKind::Down, 240.0, edge + 6.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 300.0, edge + 2.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 360.0, edge + 4.0)).unwrap();
    assert_eq!(s.tool.ink_preview.len(), 2, "the live preview is the snapped line");
    s.pointer(ev(PointerKind::Up, 360.0, edge + 3.0)).unwrap();
    let strokes = ink_strokes(&s);
    assert_eq!(strokes.len(), 1);
    let line = &strokes[0];
    assert_eq!(line.len(), 2, "committed as one straight line: {line:?}");
    assert!((line[0].1 - edge).abs() < 1e-6 && (line[1].1 - edge).abs() < 1e-6, "{line:?}");
    assert!((line[0].0 - 240.0).abs() < 1e-6 && (line[1].0 - 360.0).abs() < 1e-6, "extent kept: {line:?}");
    // The stroke's colour and width are the pen's.
    let st = s.doc().unwrap();
    let model::ShapeKind::Ink { strokes } = &st.shapes().iter().find(|x| matches!(x.kind, model::ShapeKind::Ink { .. })).unwrap().kind else {
        panic!("ink")
    };
    assert_eq!(strokes[0].width, 3.0);
    assert_eq!(strokes[0].color, model::Rgba::rgb(10, 10, 10));
    assert!(!strokes[0].highlighter);
}

#[test]
fn ink_away_from_the_ruler_stays_freehand() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 100, "angle": 0})).unwrap();
    pen(&mut s);
    s.pointer(ev(PointerKind::Down, 100.0, 400.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 200.0, 420.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 300.0, 440.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 300.0, 440.0)).unwrap();
    let strokes = ink_strokes(&s);
    assert_eq!(strokes[0].len(), 3, "{strokes:?}");
    // Hiding the ruler stops the snapping too.
    s.execute("draw.ruler", &json!({"on": false})).unwrap();
    s.pointer(ev(PointerKind::Down, 240.0, 106.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 300.0, 100.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 360.0, 95.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 360.0, 95.0)).unwrap();
    let strokes = ink_strokes(&s);
    assert_eq!(strokes[1].len(), 3, "{strokes:?}");
}

#[test]
fn ink_that_crosses_the_ruler_is_not_straightened() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 300, "angle": 0})).unwrap();
    pen(&mut s);
    // A stroke over the body and out both sides: it touches each edge once, but follows neither.
    s.pointer(ev(PointerKind::Down, 240.0, 200.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 300.0, 330.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 360.0, 200.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 360.0, 200.0)).unwrap();
    let strokes = ink_strokes(&s);
    assert!(strokes[0].len() > 2, "crossing ink keeps its shape: {strokes:?}");
}

fn rect(s: &mut Session, r: [f64; 4]) -> u64 {
    s.execute("shape.insert", &json!({"preset": "rect", "rect": r})).unwrap()["id"].as_u64().unwrap()
}

fn box_of(s: &Session, id: u64) -> deckcraft_geom::Xfrm {
    s.doc().unwrap().shapes().iter().find(|x| u64::from(x.id.0) == id).and_then(|x| x.xfrm).unwrap()
}

#[test]
fn a_dragged_object_snaps_its_bounding_box_to_a_ruler_edge() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 300, "angle": 0})).unwrap();
    let id = rect(&mut s, [100.0, 100.0, 100.0, 100.0]);
    // Drag until the bottom edge is 3 pt above the upper ruler edge (y = 300 - HALF_WIDTH).
    let top_edge = 300.0 - HALF_WIDTH;
    let want_dy = top_edge - 3.0 - 200.0;
    s.pointer(ev(PointerKind::Down, 150.0, 150.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 150.0, 150.0 + want_dy)).unwrap();
    s.pointer(ev(PointerKind::Up, 150.0, 150.0 + want_dy)).unwrap();
    let b = box_of(&s, id);
    assert!((b.y + b.h - top_edge).abs() < 1e-6, "the bottom edge sits on the ruler edge: {b:?}");
    // The same shape away from the ruler does not move to it.
    let id2 = rect(&mut s, [100.0, 400.0, 100.0, 100.0]);
    s.pointer(ev(PointerKind::Down, 150.0, 450.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 150.0, 478.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 150.0, 478.0)).unwrap();
    let b2 = box_of(&s, id2);
    assert!((b2.y - 428.0).abs() < 1e-6, "free drag: {b2:?}");
}

#[test]
fn a_multi_selection_snaps_as_one_set() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 300, "angle": 0})).unwrap();
    let a = rect(&mut s, [100.0, 100.0, 100.0, 80.0]);
    let b = rect(&mut s, [300.0, 100.0, 100.0, 80.0]);
    s.execute("edit.select", &json!({"ids": [a, b]})).unwrap();
    // The set's bottom is at 180; drag so it comes 4 pt above the upper edge (274).
    let top_edge = 300.0 - HALF_WIDTH;
    let want_dy = top_edge - 4.0 - 180.0;
    s.pointer(ev(PointerKind::Down, 150.0, 140.0)).unwrap();
    s.pointer(ev(PointerKind::Drag, 150.0, 140.0 + want_dy)).unwrap();
    s.pointer(ev(PointerKind::Up, 150.0, 140.0 + want_dy)).unwrap();
    let (ba, bb) = (box_of(&s, a), box_of(&s, b));
    assert!((ba.y + ba.h - top_edge).abs() < 1e-6, "the set's bottom edge is on the ruler: {ba:?}");
    assert!((bb.y - ba.y).abs() < 1e-6, "both moved together: {bb:?}");
    assert!((bb.x - ba.x - 200.0).abs() < 1e-6, "the set keeps its spacing: {bb:?}");
}

#[test]
fn ctrl_during_a_drag_turns_ruler_snapping_off() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 300, "angle": 0})).unwrap();
    let id = rect(&mut s, [100.0, 100.0, 100.0, 100.0]);
    let top_edge = 300.0 - HALF_WIDTH;
    let want_dy = top_edge - 3.0 - 200.0;
    // Ctrl is held during the drag, not the press: Ctrl+press toggles the selection, as in PowerPoint.
    let ev2 = |kind, x, y| PointerEvent { kind, x, y, mods: Mods { cmd: true, ..Mods::default() }, tol: 3.0, pressure: None };
    s.pointer(ev(PointerKind::Down, 150.0, 150.0)).unwrap();
    s.pointer(ev2(PointerKind::Drag, 150.0, 150.0 + want_dy)).unwrap();
    s.pointer(ev(PointerKind::Up, 150.0, 150.0 + want_dy)).unwrap();
    let b = box_of(&s, id);
    assert!((b.y - (100.0 + want_dy)).abs() < 1e-6, "Ctrl keeps the raw drag: {b:?}");
}

#[test]
fn a_shape_snaps_to_a_turned_ruler_by_its_own_edge_not_its_box() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 300, "angle": 45})).unwrap();
    // An oval is a circle inside its box: with a turned ruler its own edge meets the line a long
    // way inside the box's corner, so a box snap would leave a visible gap.
    let id = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [400.0, 60.0, 100.0, 100.0]})).unwrap()["id"].as_u64().unwrap();
    let r = s.view.ruler;
    let [edge, _] = crate::ruler::edges(&r);
    let (n, c0) = (edge.normal, deckcraft_geom::Point::new(450.0, 110.0));
    let radius = 50.0;
    let line = (edge.from.x * n.x) + (edge.from.y * n.y);
    let along = (c0.x * n.x + c0.y * n.y) - line;
    // Drag the circle so its silhouette comes 3 pt short of the edge (the box corner is nearer).
    let want = radius + 3.0;
    let dx = n.x * (want - along);
    let dy = n.y * (want - along);
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    s.pointer(ev(PointerKind::Down, c0.x, c0.y)).unwrap();
    s.pointer(ev(PointerKind::Drag, c0.x + dx, c0.y + dy)).unwrap();
    s.pointer(ev(PointerKind::Up, c0.x + dx, c0.y + dy)).unwrap();
    let b = box_of(&s, id);
    let c = deckcraft_geom::Point::new(b.x + b.w / 2.0, b.y + b.h / 2.0);
    let gap = (c.x * n.x + c.y * n.y) - line;
    assert!((gap - radius).abs() < 1e-6, "the oval's own edge sits on the ruler, not its box corner: {gap} vs {radius}");
}

#[test]
fn shift_keeps_a_drag_on_its_axis_when_a_turned_ruler_snaps() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 300, "angle": 315})).unwrap();
    let id = rect(&mut s, [100.0, 100.0, 100.0, 100.0]);
    let r = s.view.ruler;
    let [edge, _] = crate::ruler::edges(&r);
    let (n, c0) = (edge.normal, deckcraft_geom::Point::new(150.0, 150.0));
    let line = edge.from.x * n.x + edge.from.y * n.y;
    let reach = 50.0 * n.x.abs() + 50.0 * n.y.abs();
    // A horizontal drag (Shift, so y stays) that brings the box 3 pt short of the edge.
    let want = reach + 3.0;
    let dx = (line + want - (c0.x * n.x + c0.y * n.y)) / n.x;
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    let shift = |kind, x, y| PointerEvent { kind, x, y, mods: Mods { shift: true, ..Mods::default() }, tol: 3.0, pressure: None };
    // Shift is held during the drag, not the press: a Shift press toggles the selection.
    s.pointer(ev(PointerKind::Down, c0.x, c0.y)).unwrap();
    s.pointer(shift(PointerKind::Drag, c0.x + dx / 2.0, c0.y)).unwrap();
    s.pointer(shift(PointerKind::Drag, c0.x + dx, c0.y)).unwrap();
    s.pointer(shift(PointerKind::Up, c0.x + dx, c0.y)).unwrap();
    let b = box_of(&s, id);
    assert!((b.y - 100.0).abs() < 1e-6, "Shift kept the drag on its axis: {b:?}");
    // The edge's pull is along its normal; only its part along the axis is taken, so the drag
    // still stays on the axis instead of being carried off it.
    assert!((b.x - (100.0 + dx - 3.0 * n.x)).abs() < 0.01, "the edge pulls along the axis only: {b:?} dx {dx}");
}

#[test]
fn only_left_and_right_turn_the_ruler() {
    let mut s = blank();
    s.execute("draw.ruler", &json!({"on": true, "x": 300, "y": 200, "angle": 0})).unwrap();
    s.key("F6", Mods { shift: true, ..Mods::default() }).unwrap();
    // A selected shape would be the one an unused Alt+arrow moved if they fell through.
    let id = rect(&mut s, [100.0, 100.0, 100.0, 50.0]);
    let ev = |kind, x, y| PointerEvent { kind, x, y, mods: Mods::default(), tol: 3.0, pressure: None };
    s.pointer(ev(PointerKind::Down, 150.0, 125.0)).unwrap();
    s.pointer(ev(PointerKind::Up, 150.0, 125.0)).unwrap();
    let before = box_of(&s, id);
    let page = s.doc().unwrap().selection.slide;
    for k in ["Up", "Down"] {
        let r = s.key(k, Mods { alt: true, ..Mods::default() }).unwrap();
        assert_eq!(r["handled"], true, "manipulation mode owns Alt+{k}");
    }
    assert_eq!(ruler(&mut s)["angle"].as_f64(), Some(0.0), "Alt+Up/Down do not turn it");
    assert_eq!((ruler(&mut s)["x"].as_f64(), ruler(&mut s)["y"].as_f64()), (Some(300.0), Some(200.0)));
    assert_eq!(box_of(&s, id), before, "nor do they nudge the selection");
    // With nothing selected they would otherwise step the slide deck.
    s.execute("edit.select", &json!({"ids": []})).unwrap();
    let r = s.key("Up", Mods { alt: true, ..Mods::default() }).unwrap();
    assert_eq!(r["handled"], true);
    assert_eq!(s.doc().unwrap().selection.slide, page, "nor turn the page");
    s.key("Left", Mods { alt: true, ..Mods::default() }).unwrap();
    assert_eq!(ruler(&mut s)["angle"].as_f64(), Some(345.0));
}
