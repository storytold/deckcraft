//! Picture Format ▸ Picture Styles: the ribbon row gives way to Crop and Size, and its arrow opens
//! every style.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use deckcraft_engine::cmd::picture_style::STYLES;
use deckcraft_model::{Geom, ShapeId};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{Services, SlideApp};

/// A blank slide with one selected picture, on the Picture Format tab, in a `width` px window.
fn picture_format(width: f32) -> (Harness<'static, SlideApp>, ShapeId) {
    let mut s = Session::with_new();
    s.execute("slide.layout", &json!({"layout": "blank"})).unwrap();
    let pixels = (0..4).flat_map(|_| [40, 120, 200, 255]).collect();
    let png = deckcraft_render::Image { width: 2, height: 2, pixels }.to_png();
    let data = deckcraft_engine::cmd::base64_encode(&png);
    let id = s.execute("insert.picture", &json!({"data": data, "name": "a.png", "rect": [200, 100, 400, 250]})).unwrap()["id"].as_u64().unwrap();
    s.execute("edit.select", &json!({"ids": [id]})).unwrap();
    let mut app = SlideApp::new(s, Services::default());
    app.ui.tab = "Picture Format".into();
    let mut h = Harness::builder().with_size(egui::vec2(width, 900.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    (h, ShapeId(u32::try_from(id).unwrap()))
}

/// Picture Styles tiles in the ribbon row (the More menu is closed).
fn inline_styles(h: &Harness<'_, SlideApp>) -> usize {
    STYLES.iter().filter(|(_, label)| h.query_by_label(label).is_some()).count()
}

#[test]
fn crop_and_size_stay_on_screen_beside_the_picture_styles() {
    // The desktop app opens 1440 px wide. The row does not fit there, so Arrange, Crop and Size
    // fold to their one-row drop-downs: still on screen, no scroll arrow needed. At 1920 px they
    // are as designed, with the height and width boxes in the row.
    let (mut h, _) = picture_format(1440.0);
    assert!(h.query_by_label("Scroll ribbon right").is_none(), "the folded row fits at 1440 px");
    for label in ["Crop", "Size"] {
        let r = h.get_by_label(label).rect();
        assert!(r.max.x <= 1440.0 && r.min.x >= 0.0, "{label} drop-down is on screen at 1440 px: {r:?}");
    }
    assert!(h.query_by_label("Height value").is_none(), "Size's boxes are in its drop-down at 1440 px");
    h.get_by_label("Size").click();
    h.run_steps(2);
    for label in ["Height value", "Width value"] {
        assert!(h.query_by_label(label).is_some(), "{label} is in the Size drop-down");
    }

    let (h, _) = picture_format(1920.0);
    let edge = h.query_by_label("Scroll ribbon right").map_or(1920.0, |n| n.rect().min.x);
    for label in ["Crop", "Height value", "Width value"] {
        let r = h.get_by_label(label).rect();
        assert!(r.max.x <= edge, "{label} at {r:?} is past the ribbon's edge ({edge}) in a 1920 px window");
    }
    assert!(h.query_by_label("Scroll ribbon right").is_none(), "the whole tab fits at 1920 px");
}

#[test]
fn the_styles_row_grows_with_the_window_and_keeps_two_when_it_is_narrow() {
    let shown: Vec<usize> = [1280.0, 1440.0, 1920.0, 2600.0, 3200.0].map(|w| inline_styles(&picture_format(w).0)).to_vec();
    assert!(shown.windows(2).all(|p| p[0] <= p[1]), "{shown:?}");
    assert!(shown[0] >= 2, "at least two styles in a narrow window: {shown:?}");
    assert!(shown[2] > shown[1], "{shown:?}");
    assert_eq!(shown[4], STYLES.len(), "a wide window shows every style: {shown:?}");

    // Narrow, the groups after the gallery fold, so Crop and Size stay on screen without a
    // scroll arrow; the gallery visibly gives way to them.
    let (h, _) = picture_format(1280.0);
    assert!(h.query_by_label("Scroll ribbon right").is_none(), "the folded row fits at 1280 px");
    assert!(h.get_by_label("Crop").rect().max.x <= 1280.0, "Crop is on screen at 1280 px");
}

#[test]
fn more_picture_styles_lists_every_style_and_applies_the_one_clicked() {
    let (mut h, id) = picture_format(1440.0);
    let (_, last) = STYLES[STYLES.len() - 1];
    assert!(h.query_by_label(last).is_none(), "{last} is not in the row at 1440 px");
    let undo = h.state().session.active().unwrap().history.undo.len();

    h.get_by_label("More Picture Styles").click();
    h.run_steps(2);
    // The menu has every style, also the ones the row shows.
    for (_, label) in STYLES {
        assert!(h.query_all_by_label(label).count() >= 1, "{label} is missing from More Picture Styles");
    }
    h.get_by_label(last).click();
    h.run_steps(2);

    let doc = h.state().session.active().unwrap();
    assert!(matches!(&doc.shape(id).unwrap().geom, Geom::Preset { name, .. } if name == "ellipse"), "Metal Oval makes the picture an oval");
    assert_eq!(doc.history.undo.len(), undo + 1, "one undo step");
    assert!(h.query_by_label(last).is_none(), "the menu closes after a pick");
}

#[test]
fn a_resize_alone_settles_the_styles_row() {
    // No pointer event follows the resize, so only the ribbon's own repaint request can bring the
    // row to the new width.
    let (mut h, _) = picture_format(1440.0);
    h.run_ok();
    let narrow = inline_styles(&h);
    assert!(narrow >= 2, "the gallery keeps a row of styles: {narrow}");
    h.set_size(egui::vec2(1920.0, 900.0));
    assert!(h.run_ok().is_some(), "the ribbon keeps asking for frames after a resize");
    // The exact count depends on how wide the rest of the ribbon is; it only has to grow.
    let grown = inline_styles(&h);
    assert!(grown > narrow, "the row still has the 1440 px count after growing to 1920 px");
    assert!(h.query_by_label("Scroll ribbon right").is_none());
    h.set_size(egui::vec2(1440.0, 900.0));
    h.run_ok();
    let back = inline_styles(&h);
    assert!(back < grown, "the row gives styles back after shrinking to 1440 px: {back} vs {grown}");
    assert!(back >= 2, "the gallery keeps a row of styles: {back}");
}
