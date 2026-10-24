//! Slide-show frame benchmark (ignored by default): drives the real show headlessly at
//! 1920×1080 and reports, per frame while an animation, transition or GIF plays, the CPU time of
//! the app's frame (UI, slide rendering, tessellation) and the texture bytes sent to the GPU.
//!
//! `cargo test --release -p deckcraft-ui-egui --lib show_frame_bench -- --ignored --nocapture`
//!
//! The same scenarios, built for wasm32 and run in V8, give the browser numbers in
//! `docs/benchmarks/slideshow.md`.

use std::time::Instant;

use deckcraft_geom::Xfrm;
use deckcraft_model::{Shape, ShapeId, ShapeKind, style::PictureFill};
use serde_json::json;

use super::*;

const W: f32 = 1920.0;
const H: f32 = 1080.0;

struct Bench {
    ctx: egui::Context,
    app: SlideApp,
    t: f64,
    events: Vec<egui::Event>,
}

fn last_id(v: &serde_json::Value) -> u64 {
    v.get("id").and_then(serde_json::Value::as_u64).unwrap_or(0)
}

/// A smooth, photo-like picture (our own procedural content).
fn photo(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let img = image::RgbImage::from_fn(w, h, |px, py| {
        let (fx, fy, s) = (px as f32 / w as f32, py as f32 / h as f32, seed as f32);
        let a = (fx * 7.0 + s).sin() * (fy * 5.0 - s).cos();
        let b = ((fx + fy) * 11.0 + s * 0.7).sin();
        let n = ((px.wrapping_mul(73856093) ^ py.wrapping_mul(19349663) ^ seed) % 23) as f32 / 23.0;
        let c = |v: f32| v.clamp(0.0, 255.0) as u8;
        image::Rgb([
            c(40.0 + 90.0 * fy + 60.0 * a + 12.0 * n),
            c(80.0 + 120.0 * (1.0 - fy) * (0.5 + 0.5 * b) + 12.0 * n),
            c(120.0 + 100.0 * fx + 40.0 * a + 12.0 * n),
        ])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).expect("png");
    out.into_inner()
}

/// An animated GIF: a ball crossing a gradient, `n` frames of 50 ms.
fn gif(w: u32, h: u32, n: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).expect("repeat");
        let frames = (0..n).map(|i| {
            let p = i as f32 / n as f32;
            let (cx, cy) = (w as f32 * (0.2 + 0.6 * p), h as f32 * (0.5 - 0.3 * (p * std::f32::consts::TAU).sin()));
            let buf = image::RgbaImage::from_fn(w, h, |px, py| {
                let d = ((px as f32 - cx).powi(2) + (py as f32 - cy).powi(2)).sqrt();
                if d < h as f32 * 0.15 {
                    image::Rgba([250, 200, 40, 255])
                } else {
                    image::Rgba([(px * 255 / w) as u8, 60, (py * 255 / h) as u8, 255])
                }
            });
            image::Frame::from_parts(buf, 0, 0, image::Delay::from_numer_denom_ms(50, 1))
        });
        enc.encode_frames(frames).expect("gif");
    }
    out
}

fn picture(s: &mut deckcraft_engine::Session, name: &str, mime: &str, data: Vec<u8>, xf: Xfrm, id: u32, bottom: bool) {
    s.edit(|doc, _| {
        let media = doc.add_media(name, mime, data).expect("media");
        let sl = std::sync::Arc::make_mut(&mut doc.slides[0]);
        let sh = Shape {
            id: ShapeId(id),
            xfrm: Some(xf),
            kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
            ..Default::default()
        };
        if bottom {
            sl.shapes.insert(0, sh);
        } else {
            sl.shapes.push(sh);
        }
        Ok(())
    })
    .expect("edit");
}

/// Scenario `name`: the session and the slide the show starts on.
fn scenario(name: &str) -> (deckcraft_engine::Session, usize) {
    let mut s = deckcraft_engine::Session::with_new();
    let sample = |s: &mut deckcraft_engine::Session| deckcraft_engine::sample::open_sample(s).expect("sample");
    match name {
        // Sample deck, slide 2: bullets fading in by paragraph.
        "agenda" => {
            sample(&mut s);
            (s, 1)
        }
        // Sample deck, slide 4: four shadowed shapes zooming in one after another.
        "process" => {
            sample(&mut s);
            (s, 3)
        }
        // Title slide to the agenda: a Push transition.
        "push" => {
            sample(&mut s);
            (s, 0)
        }
        // Section header to the process slide: a Morph transition.
        "morph" => {
            sample(&mut s);
            (s, 2)
        }
        // A full-slide photo, a title and five shapes flying, spinning, wiping, growing and
        // following a path at once, with shadows and a glow.
        "photo" => {
            s.execute("slide.layout", &json!({"layout": "titleOnly"})).expect("layout");
            picture(&mut s, "photo.png", "image/png", photo(1920, 1080, 3), Xfrm::new(0.0, 0.0, 960.0, 540.0), 9000, true);
            let title = s.doc().expect("doc").current_slide().and_then(|sl| sl.shapes.iter().find(|x| x.ph.is_some())).map_or(0, |x| x.id.0);
            s.execute("text.set", &json!({"id": title, "text": "Aurora over the fjord"})).expect("title");
            let specs = [
                ("roundRect", [60.0, 160.0, 220.0, 130.0], "fly", "entrance", "b"),
                ("star5", [330.0, 150.0, 160.0, 160.0], "spin", "emphasis", ""),
                ("rect", [540.0, 160.0, 360.0, 140.0], "wipe", "entrance", "l"),
                ("ellipse", [120.0, 340.0, 200.0, 150.0], "growShrink", "emphasis", ""),
                ("hexagon", [600.0, 340.0, 180.0, 150.0], "lines", "path", ""),
            ];
            for (i, (preset, r, effect, class, opt)) in specs.iter().enumerate() {
                let v = s.execute("shape.insert", &json!({"preset": preset, "rect": r, "text": format!("Point {}", i + 1)})).expect("shape");
                let id = last_id(&v);
                let color = ["accent1", "accent2", "accent4", "accent5", "accent6"].get(i).copied().unwrap_or("accent1");
                s.execute("shape.fill", &json!({"id": id, "color": color})).expect("fill");
                s.execute("shape.effects", &json!({"id": id, "shadow": "bottom"})).expect("shadow");
                if i == 1 {
                    s.execute("shape.effects", &json!({"id": id, "glow": {"color": "accent2", "radius": 8}})).expect("glow");
                }
                s.execute("edit.select", &json!({"ids": [id]})).expect("select");
                let mut p = json!({"effect": effect, "class": class, "start": if i == 0 { "onClick" } else { "withPrevious" }, "duration": 2000});
                if !opt.is_empty() {
                    p["option"] = json!(opt);
                }
                s.execute("animation.add", &p).expect("animation");
            }
            s.execute("edit.deselect", &json!({})).expect("deselect");
            (s, 0)
        }
        // Two animated GIFs over a photo with a title.
        "gif" => {
            s.execute("slide.layout", &json!({"layout": "titleOnly"})).expect("layout");
            picture(&mut s, "photo.png", "image/png", photo(1920, 1080, 5), Xfrm::new(0.0, 0.0, 960.0, 540.0), 9000, true);
            let title = s.doc().expect("doc").current_slide().and_then(|sl| sl.shapes.iter().find(|x| x.ph.is_some())).map_or(0, |x| x.id.0);
            s.execute("text.set", &json!({"id": title, "text": "Watch the ball"})).expect("title");
            picture(&mut s, "a.gif", "image/gif", gif(480, 360, 24), Xfrm::new(60.0, 150.0, 400.0, 300.0), 9001, false);
            picture(&mut s, "b.gif", "image/gif", gif(480, 360, 16), Xfrm::new(500.0, 150.0, 400.0, 300.0), 9002, false);
            (s, 0)
        }
        _ => panic!("unknown scenario {name}"),
    }
}

impl Bench {
    fn new(name: &str) -> Bench {
        let (session, slide) = scenario(name);
        let mut app = SlideApp::new(session, crate::Services::default());
        app.ui.presenter_view = false;
        let mut b = Bench { ctx: egui::Context::default(), app, t: 0.0, events: vec![] };
        for _ in 0..3 {
            b.frame();
        }
        b.app.start_show(slide, false);
        for _ in 0..30 {
            b.frame();
        }
        b
    }

    fn show(&self) -> Option<&Show> {
        self.app.show.as_ref()
    }

    fn click(&mut self) {
        for pressed in [true, false] {
            self.events.push(egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

    /// One 60 Hz frame: its CPU time (ms) and the texture bytes it uploads.
    fn frame(&mut self) -> (f64, usize) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W, H))),
            time: Some(self.t),
            predicted_dt: 1.0 / 60.0,
            max_texture_side: Some(8192),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
        let ctx = self.ctx.clone();
        let app = &mut self.app;
        let start = Instant::now();
        let out = self.ctx.run_ui(raw, |ui| {
            app.logic(&ctx);
            app.ui(ui);
        });
        let _ = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        self.t += 1.0 / 60.0;
        let bytes = out.textures_delta.set.values().flat_map(|ds| ds.iter()).map(|d| d.image.width() * d.image.height() * 4).sum();
        (ms, bytes)
    }
}

/// Frames of scenario `name` while something plays: animation steps on the starting slide (one
/// click after another), the transition after one click, or the GIFs.
fn measure(name: &str) -> (Vec<f64>, Vec<usize>) {
    let mut b = Bench::new(name);
    let (mut ms, mut up) = (vec![], vec![]);
    if name == "push" || name == "morph" {
        let next = b.show().map_or(0, |s| s.state.slide) + 1;
        let dur =
            b.app.session.active().and_then(|d| d.doc.slides.get(next).and_then(|s| s.transition.as_ref()).map(|t| t.duration_ms)).unwrap_or(1000);
        b.click();
        for _ in 0..(dur as usize * 60).div_ceil(1000).max(1) {
            let (m, u) = b.frame();
            ms.push(m);
            up.push(u);
        }
        return (ms, up);
    }
    let gifs = name == "gif";
    let start = b.show().map_or(0, |s| s.state.slide);
    let mut idle = 0;
    for _ in 0..2000 {
        if b.show().is_none_or(|s| s.state.slide != start) || ms.len() >= 180 {
            break;
        }
        if !gifs && idle > 10 {
            b.click();
            idle = 0;
        }
        let (m, u) = b.frame();
        if gifs || b.show().is_some_and(|s| s.state.playing || s.state.trigger_playing()) {
            ms.push(m);
            up.push(u);
        } else {
            idle += 1;
        }
    }
    (ms, up)
}

#[test]
#[ignore = "benchmark: run with --release --ignored --nocapture"]
fn show_frame_bench() {
    println!("scenario  frames  >16.7ms  mean ms  median   p95 ms   max ms  upload MB/frame");
    for name in ["agenda", "process", "photo", "gif", "push", "morph"] {
        let (mut ms, up) = measure(name);
        ms.sort_by(f64::total_cmp);
        let n = ms.len().max(1);
        let mean = ms.iter().sum::<f64>() / n as f64;
        let at = |q: f64| ms.get(((ms.len().saturating_sub(1)) as f64 * q).round() as usize).copied().unwrap_or(0.0);
        let over = ms.iter().filter(|m| **m > 1000.0 / 60.0).count() * 100 / n;
        let mb = up.iter().sum::<usize>() as f64 / n as f64 / 1e6;
        println!("{name:<9} {:>6} {over:>7}% {mean:>8.2} {:>7.2} {:>8.2} {:>8.2} {mb:>8.2}", ms.len(), at(0.5), at(0.95), at(1.0));
    }
}
