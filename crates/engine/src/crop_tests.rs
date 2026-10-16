//! Crop with handles on the canvas (#823): Picture Format > Crop puts black handles on the
//! picture's frame; dragging one trims that side while the picture stays where it is on the
//! slide, dragging inside moves the picture under the frame, and Esc, Enter, a click outside or a
//! double-click leave the mode.

use deckcraft_geom::{Point, Rect, Xfrm};
use deckcraft_model::ShapeKind;
use deckcraft_model::style::PictureMode;
use serde_json::json;

use crate::tools::{Mods, PointerEvent, PointerKind};
use crate::*;

const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A blank slide with a 200 x 100 pt picture at (100, 100), selected.
fn picture() -> (Session, deckcraft_model::ShapeId) {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    let id = picture_at(&mut s, [100.0, 100.0, 200.0, 100.0]);
    (s, id)
}

fn picture_at(s: &mut Session, rect: [f64; 4]) -> deckcraft_model::ShapeId {
    let data = cmd::base64_encode(PNG);
    let id = s.execute("insert.picture", &json!({"data": data, "name": "a.png", "rect": rect})).unwrap()["id"].as_u64().unwrap();
    deckcraft_model::ShapeId(id as u32)
}

fn ev(kind: PointerKind, p: Point, mods: Mods) -> PointerEvent {
    PointerEvent { kind, x: p.x, y: p.y, mods, ..Default::default() }
}

fn drag(s: &mut Session, from: Point, to: Point, mods: Mods) {
    s.pointer(ev(PointerKind::Down, from, mods)).unwrap();
    let mid = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
    s.pointer(ev(PointerKind::Drag, mid, mods)).unwrap();
    s.pointer(ev(PointerKind::Drag, to, mods)).unwrap();
    s.pointer(ev(PointerKind::Up, to, mods)).unwrap();
}

fn state(s: &Session, id: deckcraft_model::ShapeId) -> (Xfrm, [f64; 4], [f64; 4]) {
    let st = s.doc().unwrap();
    let sh = st.shape(id).unwrap();
    let x = cmd::xfrm_of(&st.doc, &st.selection, sh);
    let ShapeKind::Picture { fill } = &sh.kind else { panic!("not a picture") };
    let PictureMode::Stretch { fill_rect } = fill.mode else { panic!("not stretched") };
    (x, fill.crop, fill_rect)
}

/// The whole (uncropped) image's corners on the slide, as the renderer places it.
fn image_corners(s: &Session, id: deckcraft_model::ShapeId) -> [Point; 4] {
    let (x, crop, [fl, ft, fr, fb]) = state(s, id);
    let inner = Rect::new(fl * x.w, ft * x.h, x.w - fr * x.w, x.h - fb * x.h);
    let r = deckcraft_render::crop_dest(inner, crop);
    let a = x.affine();
    [Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)].map(|q| a * q)
}

fn same_corners(a: [Point; 4], b: [Point; 4]) -> bool {
    a.iter().zip(&b).all(|(p, q)| (*p - *q).hypot() < 1e-6)
}

/// Handle `h` (0..8: NW, N, NE, E, SE, S, SW, W) of the picture's frame, on the slide.
fn handle(s: &Session, id: deckcraft_model::ShapeId, h: usize) -> Point {
    tools::box_handles(&state(s, id).0).0[h]
}

fn set(s: &mut Session, id: deckcraft_model::ShapeId, f: impl FnOnce(&mut deckcraft_model::Shape)) {
    s.edit(|doc, sel| {
        let sh = deckcraft_model::find_shape_mut(shapes_mut(doc, sel).unwrap(), id).unwrap();
        f(sh);
        Ok(())
    })
    .unwrap();
}

fn cropping(s: &Session) -> bool {
    s.active().is_some_and(|d| s.tool.cropping(d).is_some())
}

const NONE: Mods = Mods { shift: false, alt: false, cmd: false };

#[test]
fn dragging_a_crop_handle_trims_that_side_and_keeps_the_picture_in_place() {
    let (mut s, id) = picture();
    s.execute("picture.cropMode", &json!({"on": true})).unwrap();
    let before = image_corners(&s, id);
    // The W handle 50 pt to the right: the frame loses a quarter of its width on the left.
    let w = handle(&s, id, 7);
    drag(&mut s, w, Point::new(w.x + 50.0, w.y), NONE);
    let (x, c, _) = state(&s, id);
    assert!((x.x - 150.0).abs() < 1e-6 && (x.w - 150.0).abs() < 1e-6 && (x.y - 100.0).abs() < 1e-6 && (x.h - 100.0).abs() < 1e-6, "{x:?}");
    assert!((c[0] - 0.25).abs() < 1e-9 && c[1].abs() < 1e-9 && c[2].abs() < 1e-9 && c[3].abs() < 1e-9, "{c:?}");
    assert!(same_corners(before, image_corners(&s, id)), "the picture moved on the slide");
    assert!(cropping(&s), "still cropping after a drag");
}

#[test]
fn crop_handles_on_a_rotated_flipped_picture_trim_the_side_they_sit_on() {
    let (mut s, id) = picture();
    set(&mut s, id, |sh| {
        if let Some(x) = sh.xfrm.as_mut() {
            x.rot = 30.0;
            x.flip_h = true;
        }
    });
    s.execute("picture.cropMode", &json!({"on": true})).unwrap();
    let before = image_corners(&s, id);
    // Handle 7 is the frame's local left edge; with flip_h it is drawn on the right, over the
    // image's left side. Dragging it toward the centre trims the image's left side (crop[0]).
    let h7 = handle(&s, id, 7);
    let centre = state(&s, id).0.center();
    let to = Point::new(h7.x + (centre.x - h7.x) * 0.4, h7.y + (centre.y - h7.y) * 0.4);
    assert!(h7.x > centre.x, "with flip_h the local left edge is on the right");
    drag(&mut s, h7, to, NONE);
    let (x, c, _) = state(&s, id);
    assert!(c[0] > 0.15 && c[2].abs() < 1e-9 && c[1].abs() < 1e-9 && c[3].abs() < 1e-9, "{c:?}");
    assert!(x.w < 200.0 && (x.h - 100.0).abs() < 1e-6 && x.rot == 30.0 && x.flip_h && !x.flip_v, "{x:?}");
    assert!(same_corners(before, image_corners(&s, id)), "the picture moved on the slide");
    // A corner, with Shift: the frame keeps its proportions; the picture still does not move.
    let (x0, ..) = state(&s, id);
    let h4 = handle(&s, id, 4);
    drag(&mut s, h4, Point::new(h4.x - 20.0, h4.y - 25.0), Mods { shift: true, ..NONE });
    let (x1, ..) = state(&s, id);
    assert!((x1.w / x1.h - x0.w / x0.h).abs() < 1e-6, "{x0:?} {x1:?}");
    assert!(same_corners(before, image_corners(&s, id)), "the picture moved on the slide");
}

#[test]
fn crop_handles_follow_the_fill_rect_the_picture_is_drawn_in() {
    let (mut s, id) = picture();
    set(&mut s, id, |sh| {
        if let ShapeKind::Picture { fill } = &mut sh.kind {
            fill.mode = PictureMode::Stretch { fill_rect: [0.1, 0.0, 0.1, 0.0] };
        }
    });
    s.execute("picture.cropMode", &json!({"on": true})).unwrap();
    let before = image_corners(&s, id);
    let e = handle(&s, id, 3);
    drag(&mut s, e, Point::new(e.x - 40.0, e.y), NONE);
    let (x, c, _) = state(&s, id);
    assert!((x.w - 160.0).abs() < 1e-6, "{x:?}");
    // The fill rectangle is a share of the box: shrinking the box moves the left inset from 20 to
    // 16 pt, so the left crop goes slightly negative for the image to stay put.
    assert!((c[0] + 0.025).abs() < 1e-9 && (c[2] - 0.225).abs() < 1e-9, "{c:?}");
    assert!(same_corners(before, image_corners(&s, id)), "the picture moved on the slide");
}

#[test]
fn alt_crops_both_sides_alike_and_overlapping_crops_never_turn_into_nan() {
    let (mut s, id) = picture();
    s.execute("picture.cropMode", &json!({"on": true})).unwrap();
    let w = handle(&s, id, 7);
    drag(&mut s, w, Point::new(w.x + 20.0, w.y), Mods { alt: true, ..NONE });
    let (x, c, _) = state(&s, id);
    assert!((x.w - 160.0).abs() < 1e-6 && (x.x - 120.0).abs() < 1e-6, "{x:?}");
    assert!((c[0] - 0.1).abs() < 1e-9 && (c[2] - 0.1).abs() < 1e-9, "{c:?}");

    // A file can say left 60% and right 60%: nothing is visible, and a drag stays finite.
    set(&mut s, id, |sh| {
        if let ShapeKind::Picture { fill } = &mut sh.kind {
            fill.crop = [0.6, 0.0, 0.6, 0.0];
        }
    });
    let w = handle(&s, id, 7);
    drag(&mut s, w, Point::new(w.x + 10.0, w.y), NONE);
    let c = state(&s, id).1;
    assert!(c.iter().all(|v| v.is_finite()) && c[0] + c[2] < 1.0, "{c:?}");
    let m = s.doc().unwrap().shapes().iter().find(|sh| sh.id == id).unwrap().clone();
    drag(&mut s, m.xfrm.unwrap().center(), Point::new(500.0, 300.0), NONE);
    let c = state(&s, id).1;
    assert!(c.iter().all(|v| v.is_finite()) && c[0] + c[2] < 1.0, "{c:?}");
}

#[test]
fn dragging_inside_moves_the_picture_under_the_frame() {
    let (mut s, id) = picture();
    s.execute("picture.crop", &json!({"left": 0.2, "right": 0.2})).unwrap();
    s.execute("picture.cropMode", &json!({"on": true})).unwrap();
    let (x0, ..) = state(&s, id);
    // 20 pt right: the picture (shown 200 pt wide for 60% of it) moves, the frame does not.
    let c = x0.center();
    drag(&mut s, c, Point::new(c.x + 20.0, c.y + 3.0), Mods { shift: true, ..NONE });
    let (x1, c1, _) = state(&s, id);
    assert_eq!(x0, x1);
    let iw = 200.0 / 0.6;
    assert!((c1[0] - (0.2 - 20.0 / iw)).abs() < 1e-9 && (c1[2] - (0.2 + 20.0 / iw)).abs() < 1e-9, "{c1:?}");
    assert!(c1[1].abs() < 1e-9 && c1[3].abs() < 1e-9, "Shift keeps the drag horizontal: {c1:?}");
    // Far to the left: the move stops at the limit, both sides by the same amount.
    drag(&mut s, c, Point::new(c.x - 10_000.0, c.y), NONE);
    let (_, c2, _) = state(&s, id);
    assert!((c2[0] - 0.99).abs() < 1e-9, "{c2:?}");
    assert!((c2[0] + c2[2] - 0.4).abs() < 1e-9, "{c2:?}");
}

#[test]
fn a_crop_session_is_one_undo_step() {
    let (mut s, id) = picture();
    let undo = |s: &Session| s.doc().unwrap().history.undo.len();
    s.execute("picture.cropMode", &json!({"on": true})).unwrap();
    let n = undo(&s);
    let w = handle(&s, id, 7);
    drag(&mut s, w, Point::new(w.x + 30.0, w.y), NONE);
    let e = handle(&s, id, 3);
    drag(&mut s, e, Point::new(e.x - 30.0, e.y), NONE);
    s.key("Escape", NONE).unwrap();
    assert_eq!(undo(&s), n + 1);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(state(&s, id).1, [0.0; 4]);
    assert_eq!(state(&s, id).0.w, 200.0);

    // drag, Undo, Crop 10% from the ribbon, drag: one Undo takes back the last drag only.
    s.execute("edit.select", &json!({"ids": [id.0]})).unwrap();
    s.execute("picture.cropMode", &json!({"on": true})).unwrap();
    let w = handle(&s, id, 7);
    drag(&mut s, w, Point::new(w.x + 30.0, w.y), NONE);
    s.execute("edit.undo", &json!({})).unwrap();
    s.execute("picture.crop", &json!({"left": 0.1, "top": 0.1, "right": 0.1, "bottom": 0.1})).unwrap();
    let after_ribbon = state(&s, id);
    let w = handle(&s, id, 7);
    drag(&mut s, w, Point::new(w.x + 30.0, w.y), NONE);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(state(&s, id), after_ribbon);
}

#[test]
fn esc_enter_click_outside_and_double_click_leave_crop_mode() {
    let (mut s, id) = picture();
    let on = |s: &mut Session| {
        s.execute("edit.select", &json!({"ids": [id.0]})).unwrap();
        s.execute("picture.cropMode", &json!({"on": true})).unwrap();
        assert!(cropping(s));
    };
    on(&mut s);
    s.key("Escape", NONE).unwrap();
    assert!(!cropping(&s));
    assert_eq!(s.doc().unwrap().selection.shapes, vec![id], "Esc leaves cropping, not the selection");

    on(&mut s);
    s.key("Enter", NONE).unwrap();
    assert!(!cropping(&s));
    assert!(s.doc().unwrap().selection.text.is_none(), "Enter does not start typing in the picture");

    on(&mut s);
    s.pointer(ev(PointerKind::Down, Point::new(600.0, 400.0), NONE)).unwrap();
    s.pointer(ev(PointerKind::Up, Point::new(600.0, 400.0), NONE)).unwrap();
    assert!(!cropping(&s) && s.tool.crop.is_none());

    on(&mut s);
    let c = state(&s, id).0.center();
    s.pointer(ev(PointerKind::Down, c, NONE)).unwrap();
    s.pointer(ev(PointerKind::Up, c, NONE)).unwrap();
    s.pointer(ev(PointerKind::Down, c, NONE)).unwrap();
    // As the canvas sends it: the second press, then the double-click instead of a release.
    s.pointer(ev(PointerKind::DoubleClick, c, NONE)).unwrap();
    assert!(!cropping(&s));
    assert!(s.ui_requests.is_empty(), "the double-click that ends cropping opens no pane");
    let n = s.doc().unwrap().history.undo.len();
    s.execute("picture.adjust", &json!({"brightness": 0.2})).unwrap();
    assert_eq!(s.doc().unwrap().history.undo.len(), n + 1, "a command after the double-click is its own undo step");

    // Another picture joins the selection: cropping is over, so Esc deselects as usual.
    let other = picture_at(&mut s, [400.0, 300.0, 100.0, 100.0]);
    on(&mut s);
    s.execute("edit.select", &json!({"ids": [other.0], "add": true})).unwrap();
    s.key("Escape", NONE).unwrap();
    assert!(s.doc().unwrap().selection.shapes.is_empty() && s.tool.crop.is_none());
    on(&mut s);
    let oc = state(&s, other).0.center();
    s.pointer(ev(PointerKind::Down, oc, Mods { shift: true, ..NONE })).unwrap();
    s.pointer(ev(PointerKind::Up, oc, Mods { shift: true, ..NONE })).unwrap();
    s.key("Escape", NONE).unwrap();
    assert!(s.doc().unwrap().selection.shapes.is_empty());
}

#[test]
fn crop_mode_needs_one_unlocked_picture() {
    let (mut s, id) = picture();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [400, 300, 50, 50]})).unwrap();
    assert!(s.execute("picture.cropMode", &json!({"on": true})).is_err(), "a rectangle has nothing to crop");
    s.execute("edit.select", &json!({"ids": [id.0]})).unwrap();
    s.execute("shape.lock", &json!({"locked": true})).unwrap();
    assert!(s.execute("picture.cropMode", &json!({"on": true})).is_err(), "a locked picture is not cropped");
    assert!(!cropping(&s));
}
