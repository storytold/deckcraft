//! `slide.colorAt`, the colour the slide is drawn with at a point: what the Eyedropper takes (#367).

use serde_json::{Value, json};

use crate::Session;

fn deck() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
    s.execute("shape.insert", &json!({"preset": "rect", "rect": [100, 100, 200, 100]})).unwrap();
    s.execute("shape.fill", &json!({"color": "#ff0000"})).unwrap();
    s
}

fn color_at(s: &mut Session, x: f64, y: f64) -> Value {
    s.execute("slide.colorAt", &json!({"x": x, "y": y})).unwrap()
}

#[test]
fn a_point_in_a_shape_gives_its_fill_and_one_beside_it_the_background() {
    let mut s = deck();
    let inside = color_at(&mut s, 150.0, 150.0);
    assert_eq!(inside["color"], "FF0000");
    assert_eq!(inside["alpha"], 255);
    let beside = color_at(&mut s, 700.0, 450.0);
    assert_ne!(beside["color"], "FF0000");
    // Just inside and just outside the shape's corner.
    assert_eq!(color_at(&mut s, 101.0, 101.0)["color"], "FF0000");
    assert_eq!(color_at(&mut s, 99.0, 99.0)["color"], beside["color"]);
    // A finer or coarser picture gives the same colour.
    for scale in [0.5, 1.0, 4.0] {
        let v = s.execute("slide.colorAt", &json!({"x": 150, "y": 150, "scale": scale})).unwrap();
        assert_eq!(v["color"], "FF0000", "scale {scale}");
    }
    // It is a query: the deck is not changed and nothing goes on the undo stack.
    let revision = s.doc().unwrap().revision;
    color_at(&mut s, 150.0, 150.0);
    assert_eq!(s.doc().unwrap().revision, revision);
}

#[test]
fn a_see_through_fill_gives_the_colour_it_is_drawn_with() {
    let mut s = deck();
    let background = color_at(&mut s, 700.0, 450.0)["color"].as_str().unwrap().to_string();
    s.execute("shape.fill", &json!({"transparency": 0.5})).unwrap();
    let v = color_at(&mut s, 150.0, 150.0);
    let c = v["color"].as_str().unwrap();
    assert_ne!(c, "FF0000", "half of the background shows through");
    assert_ne!(c, background);
    assert_eq!(v["alpha"], 255, "the slide is opaque");
}

#[test]
fn points_outside_the_slide_and_bad_parameters_are_errors() {
    let mut s = deck();
    for p in [
        json!({"x": -1, "y": 10}),
        json!({"x": 10, "y": -0.5}),
        json!({"x": 1e9, "y": 10}),
        json!({"x": 10, "y": 1e9}),
        json!({"x": 10}),
        json!({"y": 10}),
        json!({"x": "10", "y": 10}),
        json!({"x": 10, "y": 10, "slide": 99}),
    ] {
        assert!(s.execute("slide.colorAt", &p).is_err(), "{p}");
    }
    // Odd scales are clamped, not refused or crashed on.
    for scale in [0.0, -3.0, 1e9] {
        let v = s.execute("slide.colorAt", &json!({"x": 150, "y": 150, "scale": scale}));
        assert!(v.is_ok(), "{scale}: {v:?}");
    }
}
