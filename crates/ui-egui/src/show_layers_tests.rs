//! Composited slide-show frames (`show_layers`) look like whole-slide renders of the same moment.
//!
//! egui's output is drawn here by a small software rasterizer (bilinear texture sampling,
//! premultiplied source-over, as the GPU does), and compared with `render_frame`.

use std::collections::HashMap;

use deckcraft_model::ShapeId;
use egui::{Color32, ColorImage, TextureId, epaint::Primitive};
use serde_json::json;

use super::*;

const W: f32 = 960.0;
const H: f32 = 540.0;

struct Rig {
    ctx: egui::Context,
    app: SlideApp,
    t: f64,
    textures: HashMap<TextureId, ColorImage>,
    events: Vec<egui::Event>,
    picture: Vec<[f32; 4]>,
}

impl Rig {
    fn new(session: deckcraft_engine::Session, slide: usize) -> Rig {
        let mut app = SlideApp::new(session, crate::Services::default());
        app.ui.presenter_view = false;
        let mut r = Rig { ctx: egui::Context::default(), app, t: 0.0, textures: HashMap::new(), events: vec![], picture: vec![] };
        r.frames(3);
        r.app.start_show(slide, false);
        // Past the time the show's toolbar shows after the pointer last moved.
        r.frames(240);
        r
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
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

    fn frame(&mut self) {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W, H))),
            time: Some(self.t),
            predicted_dt: 1.0 / 60.0,
            max_texture_side: Some(4096),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(1.0);
        let ctx = self.ctx.clone();
        let app = &mut self.app;
        let out = self.ctx.run_ui(raw, |ui| {
            app.logic(&ctx);
            app.ui(ui);
        });
        for (id, delta) in out.textures_delta.set.iter().flat_map(|(id, ds)| ds.iter().map(move |d| (id, d))) {
            let egui::ImageData::Color(img) = &delta.image;
            match delta.pos {
                None => {
                    self.textures.insert(*id, ColorImage::clone(img));
                }
                Some([x, y]) => {
                    if let Some(t) = self.textures.get_mut(id) {
                        for row in 0..img.size[1] {
                            for col in 0..img.size[0] {
                                if let (Some(dst), Some(src)) =
                                    (t.pixels.get_mut((y + row) * t.size[0] + x + col), img.pixels.get(row * img.size[0] + col))
                                {
                                    *dst = *src;
                                }
                            }
                        }
                    }
                }
            }
        }
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        self.picture = self.raster(&prims);
        for id in &out.textures_delta.free {
            self.textures.remove(id);
        }
        self.t += 1.0 / 60.0;
    }

    /// Bilinear sample of texture `t` at `uv` (premultiplied, 0–255).
    fn sample(t: &ColorImage, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (t.size[0] as i64, t.size[1] as i64);
        let (x, y) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let px = |xi: i64, yi: i64| {
            let c = t.pixels.get((yi.clamp(0, h - 1) * w + xi.clamp(0, w - 1)) as usize).copied().unwrap_or(Color32::TRANSPARENT);
            [c.r() as f32, c.g() as f32, c.b() as f32, c.a() as f32]
        };
        let (a, b, c, d) = (px(x0 as i64, y0 as i64), px(x0 as i64 + 1, y0 as i64), px(x0 as i64, y0 as i64 + 1), px(x0 as i64 + 1, y0 as i64 + 1));
        std::array::from_fn(|i| (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy)
    }

    fn raster(&self, prims: &[egui::ClippedPrimitive]) -> Vec<[f32; 4]> {
        let (w, h) = (W as usize, H as usize);
        let mut px = vec![[0.0f32; 4]; w * h];
        for p in prims {
            let Primitive::Mesh(m) = &p.primitive else { continue };
            let Some(tex) = self.textures.get(&m.texture_id) else { continue };
            for tri in m.indices.chunks_exact(3) {
                let v = [m.vertices[tri[0] as usize], m.vertices[tri[1] as usize], m.vertices[tri[2] as usize]];
                let area = (v[1].pos.x - v[0].pos.x) * (v[2].pos.y - v[0].pos.y) - (v[2].pos.x - v[0].pos.x) * (v[1].pos.y - v[0].pos.y);
                if area.abs() < 1e-9 {
                    continue;
                }
                let minx = v.iter().map(|q| q.pos.x).fold(f32::MAX, f32::min).max(p.clip_rect.min.x).max(0.0).floor() as usize;
                let maxx = v.iter().map(|q| q.pos.x).fold(f32::MIN, f32::max).min(p.clip_rect.max.x).min(W).ceil() as usize;
                let miny = v.iter().map(|q| q.pos.y).fold(f32::MAX, f32::min).max(p.clip_rect.min.y).max(0.0).floor() as usize;
                let maxy = v.iter().map(|q| q.pos.y).fold(f32::MIN, f32::max).min(p.clip_rect.max.y).min(H).ceil() as usize;
                for y in miny..maxy {
                    for x in minx..maxx {
                        let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                        if !p.clip_rect.contains(egui::pos2(cx, cy)) {
                            continue;
                        }
                        let e = |a: usize, b: usize| (v[b].pos.x - v[a].pos.x) * (cy - v[a].pos.y) - (v[b].pos.y - v[a].pos.y) * (cx - v[a].pos.x);
                        let (w0, w1, w2) = (e(1, 2) / area, e(2, 0) / area, e(0, 1) / area);
                        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                            continue;
                        }
                        let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
                        let vv = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
                        let col: [f32; 4] = std::array::from_fn(|i| {
                            let c = |q: &egui::epaint::Vertex| q.color.to_array()[i] as f32;
                            w0 * c(&v[0]) + w1 * c(&v[1]) + w2 * c(&v[2])
                        });
                        let s = Self::sample(tex, u, vv);
                        let src: [f32; 4] = std::array::from_fn(|i| s[i] * col[i] / 255.0);
                        let d = &mut px[y * w + x];
                        let k = 1.0 - src[3] / 255.0;
                        for i in 0..4 {
                            d[i] = src[i] + d[i] * k;
                        }
                    }
                }
            }
        }
        px
    }

    /// The whole-slide render of what the show shows now, as the show drew it before layers.
    fn expected(&mut self) -> ColorImage {
        let (uid, doc) = self.app.session.active().map(|d| (d.uid, d.doc.clone())).expect("doc");
        let now = self.t - 1.0 / 60.0;
        let slide = self.app.show.as_ref().map_or(0, |s| s.state.slide);
        let gifs = self.app.media.gifs(uid, &doc, slide, now, &self.app.session.gif_paused);
        let times: Vec<(ShapeId, f64)> = gifs.iter().map(|g| (g.id, g.t)).collect();
        let show = self.app.show.as_mut().expect("show");
        let t = if show.state.playing { now - show.step_start } else { 0.0 };
        let (idx, step) = (show.state.slide, show.state.step);
        let tl = show.timeline(&doc).clone();
        let (plays, stamps) = (show.state.trigger_plays(), show.state.trigger_stamps());
        render_frame(&doc, idx, tl, step, t, (W as u32, H as u32), vec![], &times, 0, &plays, &stamps, None)
    }

    fn layered(&self) -> bool {
        self.app.show.as_ref().is_some_and(|s| s.layers.as_ref().is_some_and(|(_, l)| l.ready()))
    }

    /// Mean and 99th-percentile channel difference between the composited frame and `want`, and
    /// the share of pixels differing by more than 48 in some channel.
    /// Write the composited frame and `want` as PPM files (DECKCRAFT_DUMP=dir), to look at.
    fn dump(&self, want: &ColorImage, name: &str) {
        let Ok(dir) = std::env::var("DECKCRAFT_DUMP") else { return };
        let ppm = |px: Vec<[u8; 3]>| {
            let mut b = format!("P6 {} {} 255\n", W as usize, H as usize).into_bytes();
            for p in px {
                b.extend_from_slice(&p);
            }
            b
        };
        let got = ppm(self.picture.iter().map(|p| [p[0] as u8, p[1] as u8, p[2] as u8]).collect());
        let wanted = ppm(want.pixels.iter().map(|c| [c.r(), c.g(), c.b()]).collect());
        let _ = std::fs::write(format!("{dir}/{name}-got.ppm"), got);
        let _ = std::fs::write(format!("{dir}/{name}-want.ppm"), wanted);
    }

    fn diff(&self, want: &ColorImage) -> (f32, f32, f32) {
        self.dump(want, &format!("t{:.3}", self.t));
        let mut d: Vec<f32> = vec![];
        let mut far = 0usize;
        for (got, w) in self.picture.iter().zip(&want.pixels) {
            let m = (0..4).map(|i| (got[i] - w.to_array()[i] as f32).abs()).fold(0.0, f32::max);
            if m > 48.0 {
                far += 1;
            }
            d.push(m);
        }
        let mean = d.iter().sum::<f32>() / d.len().max(1) as f32;
        d.sort_by(f32::total_cmp);
        let p99 = d.get(d.len() * 99 / 100).copied().unwrap_or(0.0);
        (mean, p99, far as f32 / d.len().max(1) as f32)
    }
}

fn last_id(v: &serde_json::Value) -> u64 {
    v.get("id").and_then(serde_json::Value::as_u64).unwrap_or(0)
}

/// A slide with a picture-like gradient background shape, a title, and shapes that fly in, spin,
/// wipe, grow and follow a path at once, one with a shadow and one with a glow; then bullets
/// that fade in by paragraph on the next click.
fn moving_deck() -> deckcraft_engine::Session {
    let mut s = deckcraft_engine::Session::with_new();
    let mut x = |id: &str, p: serde_json::Value| s.execute(id, &p).expect(id);
    x("slide.layout", json!({"layout": "titleOnly"}));
    let bg = x("shape.insert", json!({"preset": "rect", "rect": [0, 0, 960, 540]}));
    x("shape.fill", json!({"id": last_id(&bg), "gradient": {"stops": [[0, "accent1"], [1, "accent2"]], "angle": 30}}));
    x("arrange.sendToBack", json!({"ids": [last_id(&bg)]}));
    let specs = [
        ("roundRect", [60.0, 160.0, 220.0, 130.0], "fly", "entrance", "b"),
        ("star5", [330.0, 150.0, 160.0, 160.0], "spin", "emphasis", ""),
        ("rect", [540.0, 160.0, 360.0, 140.0], "wipe", "entrance", "l"),
        ("ellipse", [120.0, 340.0, 200.0, 150.0], "growShrink", "emphasis", ""),
        ("hexagon", [600.0, 340.0, 180.0, 150.0], "lines", "path", ""),
    ];
    for (i, (preset, r, effect, class, opt)) in specs.iter().enumerate() {
        let v = x("shape.insert", json!({"preset": preset, "rect": r, "text": format!("Point {}", i + 1)}));
        let id = last_id(&v);
        if i == 0 {
            x("shape.effects", json!({"id": id, "shadow": "bottom"}));
        }
        if i == 1 {
            x("shape.effects", json!({"id": id, "glow": {"color": "accent2", "radius": 8}}));
        }
        x("edit.select", json!({"ids": [id]}));
        let mut p = json!({"effect": effect, "class": class, "start": if i == 0 { "onClick" } else { "withPrevious" }, "duration": 1000});
        if !opt.is_empty() {
            p["option"] = json!(opt);
        }
        x("animation.add", p);
    }
    let tb = x("insert.textBox", json!({"rect": [380, 440, 500, 90], "text": "First point\nSecond point\nThird point"}));
    x("edit.select", json!({"ids": [last_id(&tb)]}));
    x("animation.add", json!({"effect": "fly", "class": "entrance", "option": "b", "start": "onClick", "duration": 1000}));
    x("animation.options", json!({"textBuild": "byParagraph"}));
    x("edit.deselect", json!({}));
    x("slide.new", json!({"layout": "blank"}));
    x("transition.set", json!({"kind": "fade"}));
    s
}

#[test]
fn composited_frames_match_whole_slide_renders() {
    let mut r = Rig::new(moving_deck(), 0);
    assert!(r.layered(), "a slide with animations is composited from layers");
    // At rest before the first click: pictures drawn as they are, so the frame matches exactly.
    let (mean, p99, far) = {
        let want = r.expected();
        r.diff(&want)
    };
    assert!(mean < 0.5 && p99 < 3.0 && far < 0.0005, "resting frame: mean {mean}, p99 {p99}, far {far}");
    r.click();
    for k in 1..=7 {
        r.frames(8);
        let want = r.expected();
        let (mean, p99, far) = r.diff(&want);
        // Moving pictures are resampled: edges and text soften a little, nothing more.
        assert!(mean < 2.5 && p99 < 40.0 && far < 0.01, "frame {k} of the step: mean {mean}, p99 {p99}, far {far}");
    }
    // At rest after the step: poses are drawn again exactly (the grown ellipse, the moved hexagon).
    r.frames(30);
    let (mean, p99, far) = {
        let want = r.expected();
        r.diff(&want)
    };
    assert!(mean < 0.5 && p99 < 3.0 && far < 0.0005, "resting after the step: mean {mean}, p99 {p99}, far {far}");
    // Paragraphs flying in one by one.
    for _ in 0..3 {
        r.click();
        for k in 1..=3 {
            r.frames(12);
            let (mean, p99, far) = {
                let want = r.expected();
                r.diff(&want)
            };
            assert!(mean < 2.5 && p99 < 40.0 && far < 0.01, "paragraph frame {k}: mean {mean}, p99 {p99}, far {far}");
        }
        r.frames(30);
    }
    let (mean, p99, far) = {
        let want = r.expected();
        r.diff(&want)
    };
    assert!(mean < 0.5 && p99 < 3.0 && far < 0.0005, "all paragraphs in: mean {mean}, p99 {p99}, far {far}");
    let _ = ShapeId(0);
}

/// Leaving a composited slide right after its last animation: the transition starts from the
/// slide as it ended, not from an older whole-slide picture.
#[test]
fn a_transition_starts_from_the_composited_slide_as_it_ended() {
    let mut r = Rig::new(moving_deck(), 0);
    for _ in 0..4 {
        r.click();
        r.frames(80);
    }
    r.frames(3);
    let want = r.expected();
    r.click();
    r.frame();
    let show = r.app.show.as_ref().expect("show");
    assert_eq!(show.state.slide, 1, "the click moved on");
    let old = show.trans.as_ref().and_then(|t| t.old.as_ref()).expect("the fade starts from the old slide");
    let img = r.textures.get(&old.id()).expect("its picture was uploaded");
    assert_eq!(img.size, want.size);
    if let Ok(dir) = std::env::var("DECKCRAFT_DUMP") {
        let ppm = |px: &[Color32]| {
            let mut b = format!("P6 {} {} 255\n", W as usize, H as usize).into_bytes();
            for c in px {
                b.extend_from_slice(&[c.r(), c.g(), c.b()]);
            }
            b
        };
        let _ = std::fs::write(format!("{dir}/old-got.ppm"), ppm(&img.pixels));
        let _ = std::fs::write(format!("{dir}/old-want.ppm"), ppm(&want.pixels));
    }
    let diff = img
        .pixels
        .iter()
        .zip(&want.pixels)
        .map(|(a, b)| (0..4).map(|i| (a.to_array()[i] as i32 - b.to_array()[i] as i32).abs()).max().unwrap_or(0))
        .filter(|d| *d > 8)
        .count();
    assert!(diff * 1000 < want.pixels.len(), "{diff} pixels differ from the slide as it ended");
}

/// Three 2×2 frames (red, green, blue), 100 ms each.
fn three_frame_gif() -> Vec<u8> {
    let mut b = b"GIF89a\x02\x00\x02\x00\x81\x00\x00\xff\x00\x00\x00\xff\x00\x00\x00\xff\x00\x00\x00".to_vec();
    b.extend_from_slice(b"\x21\xff\x0bNETSCAPE2.0\x03\x01\x00\x00\x00");
    for index in [0u8, 1, 2] {
        b.extend_from_slice(b"\x21\xf9\x04\x00\x0a\x00\x00\x00");
        b.extend_from_slice(b"\x2c\x00\x00\x00\x00\x02\x00\x02\x00\x00");
        // LZW (min code size 2): clear, four pixels of `index`, end.
        let codes = [4u16, index as u16, index as u16, index as u16, index as u16, 5];
        let (mut acc, mut bits, mut out) = (0u32, 0u32, vec![]);
        for (k, c) in codes.iter().enumerate() {
            let width = if k >= 4 { 4 } else { 3 };
            acc |= (*c as u32) << bits;
            bits += width;
            while bits >= 8 {
                out.push((acc & 0xff) as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        if bits > 0 {
            out.push(acc as u8);
        }
        b.push(2);
        b.push(out.len() as u8);
        b.extend_from_slice(&out);
        b.push(0);
    }
    b.push(0x3b);
    b
}

#[test]
fn composited_gif_frames_match_whole_slide_renders() {
    let mut s = deckcraft_engine::Session::with_new();
    s.execute("slide.layout", &json!({"layout": "titleOnly"})).expect("layout");
    let id = ShapeId(4200);
    s.edit(|doc, _| {
        let media = doc.add_media("rgb.gif", "image/gif", three_frame_gif()).expect("gif");
        let slide = std::sync::Arc::make_mut(&mut doc.slides[0]);
        slide.shapes.push(deckcraft_model::Shape {
            id,
            xfrm: Some(deckcraft_geom::Xfrm::new(200.0, 150.0, 400.0, 300.0)),
            kind: deckcraft_model::ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media, ..Default::default() } },
            ..Default::default()
        });
        Ok(())
    })
    .expect("edit");
    let mut r = Rig::new(s, 0);
    assert!(r.layered(), "a slide with an animated GIF is composited");
    let mut seen = std::collections::HashSet::new();
    for _ in 0..12 {
        r.frames(2);
        let want = r.expected();
        let (mean, p99, far) = r.diff(&want);
        assert!(mean < 0.5 && p99 < 3.0 && far < 0.0005, "GIF frame: mean {mean}, p99 {p99}, far {far}");
        let mid = r.picture[(300 * W as usize) + 400];
        seen.insert((mid[0] as u8 / 128, mid[1] as u8 / 128, mid[2] as u8 / 128));
    }
    assert!(seen.len() >= 3, "the GIF played through its frames: {seen:?}");
}
