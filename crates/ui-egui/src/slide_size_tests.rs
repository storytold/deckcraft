//! Slide sizes at the edges of what a loaded file can give (after `deckcraft_format::repair`):
//! the editor, sorter, slide show and presenter view open without panicking. (NaN and infinity
//! are saved as null, which used to make the file fail to open.)
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::json;

use crate::{Services, SlideApp};
use deckcraft_engine::Session;

fn open_with_size(w: f64, h: f64) -> egui_kittest::Harness<'static, SlideApp> {
    // Written with the size, then opened the way File › Open does (load, then repair).
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({})).unwrap();
    std::sync::Arc::make_mut(&mut s.active_mut().unwrap().doc).slide_size = deckcraft_model::Size::new(w, h);
    let path = std::env::temp_dir().join(format!("deckcraft-size-{}-{w}x{h}.deckcraft", std::process::id()));
    s.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    let mut s = Session::new();
    s.execute("file.open", &json!({"path": path.to_string_lossy()})).unwrap();
    let _ = std::fs::remove_file(&path);
    egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(s, Services::default()),
    )
}

#[test]
fn extreme_loaded_slide_sizes_open_in_every_view() {
    for (w, h) in [(0.0, 0.0), (-5.0, 540.0), (f64::NAN, 540.0), (f64::INFINITY, 1.0), (1.0, 1.0), (1e9, 1e9), (4000.0, 4000.0)] {
        let mut hs = open_with_size(w, h);
        hs.run_steps(3);
        let size = hs.state().session.active().unwrap().doc.slide_size;
        assert!((72.0..=4032.0).contains(&size.width) && (72.0..=4032.0).contains(&size.height), "{w}x{h}: {size:?}");
        for cmd in ["view.sorter", "view.normal"] {
            hs.state_mut().run(cmd, json!({})).unwrap();
            hs.run_steps(2);
        }
        hs.state_mut().run("show.presenter", json!({"on": true})).unwrap();
        hs.state_mut().run("show.start", json!({"from": 0})).unwrap();
        hs.run_steps(3);
        assert!(hs.state().show.is_some(), "{w}x{h}");
    }
}

/// A presentation whose size went bad in memory (not through a file) still lays out: the
/// canvas fit stays finite and positive.
#[test]
fn the_canvas_fit_survives_a_zero_slide_size_in_memory() {
    let mut s = Session::with_new();
    std::sync::Arc::make_mut(&mut s.active_mut().unwrap().doc).slide_size = deckcraft_model::Size::new(0.0, f64::NAN);
    let app = SlideApp::new(s, Services::default());
    let (r, scale) = crate::canvas::layout(&app, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0)));
    assert!(scale.is_finite() && scale > 0.0, "{scale}");
    assert!(r.min.x.is_finite() && r.max.y.is_finite(), "{r:?}");
}
