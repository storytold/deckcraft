use super::zoom::GRID_CAP;
use super::{
    BackKey, Color32, Key, Layer, Mesh, Prefetch, Presentation, Rect, RenderOpts, ShapeId, Show, ShowState, StepStamp, TextureHandle, Timeline,
    TriggerPlay, pos2, state_for, vec2,
};

pub(super) fn render_threads() -> u16 {
    if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) }
}

pub(super) fn render_backdrop(doc: &Presentation, key: BackKey, threads: u16) -> deckcraft_render::Image {
    let opts = RenderOpts { scale: key.size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(key.size), threads, ..Default::default() };
    deckcraft_render::render_slide_part(doc, key.slide, deckcraft_render::SlidePart::Backdrop, &opts)
}

/// `top` laid over `back` (both premultiplied RGBA of the same size; `top` alone otherwise).
fn over(back: &deckcraft_render::Image, top: &deckcraft_render::Image) -> egui::ColorImage {
    if back.width != top.width || back.height != top.height || back.pixels.len() != top.pixels.len() {
        return crate::textures::to_color_image(top, false);
    }
    let px = back
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(top.pixels.as_chunks::<4>().0)
        .map(|(b, t)| {
            let k = 255 - t[3] as u32;
            let mix = |i: usize| (t[i] as u32 + (b[i] as u32 * k + 127) / 255).min(255) as u8;
            Color32::from_rgba_premultiplied(mix(0), mix(1), mix(2), mix(3))
        })
        .collect();
    egui::ColorImage::new([top.width as usize, top.height as usize], px)
}

#[allow(clippy::too_many_arguments)]
/// Slide `idx` at `step`, `t` seconds into it, `size` pixels, with `hidden` media hidden. With a
/// `backdrop` (of this slide, at this size), only the slide's shapes are rendered, over it.
pub(super) fn render_frame(
    doc: &Presentation,
    idx: usize,
    tl: Timeline,
    step: usize,
    t: f64,
    size: (u32, u32),
    hidden: Vec<ShapeId>,
    times: &[(ShapeId, f64)],
    threads: u16,
    plays: &[TriggerPlay],
    stamps: &[StepStamp],
    backdrop: Option<&deckcraft_render::Image>,
) -> egui::ColorImage {
    let f = move |id: ShapeId| {
        let mut st = state_for(&tl, id, step, t, plays, stamps);
        if hidden.contains(&id) {
            st.visible = false;
        }
        st
    };
    let opts = RenderOpts {
        scale: size.0 as f64 / doc.slide_size.width.max(1.0),
        size: Some(size),
        state: Some(&f),
        gif_times: times,
        threads,
        ..Default::default()
    };
    match backdrop {
        Some(back) => over(back, &deckcraft_render::render_slide_part(doc, idx, deckcraft_render::SlidePart::Content, &opts)),
        None => crate::textures::to_color_image(&deckcraft_render::render_slide(doc, idx, &opts), false),
    }
}

/// Render only the slide's own shapes except `skip`, over transparency: what is stacked above a
/// playing video, to be drawn over the video frame.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_overlay(
    doc: &Presentation,
    idx: usize,
    tl: Timeline,
    step: usize,
    t: f64,
    size: (u32, u32),
    hidden: Vec<ShapeId>,
    skip: &[ShapeId],
    times: &[(ShapeId, f64)],
    threads: u16,
    plays: &[TriggerPlay],
    stamps: &[StepStamp],
) -> egui::ColorImage {
    let f = move |id: ShapeId| {
        let mut st = state_for(&tl, id, step, t, plays, stamps);
        if hidden.contains(&id) {
            st.visible = false;
        }
        st
    };
    let opts = RenderOpts {
        scale: size.0 as f64 / doc.slide_size.width.max(1.0),
        size: Some(size),
        state: Some(&f),
        skip,
        gif_times: times,
        threads,
        ..Default::default()
    };
    let img = deckcraft_render::render_slide_part(doc, idx, deckcraft_render::SlidePart::Content, &opts);
    crate::textures::to_color_image(&img, false)
}

/// The shapes of slide `idx` to leave out when drawing what is stacked above the lowest of the
/// playing `videos` (that video, everything below it and the other videos). Empty when nothing
/// is above a video: the video frame is then painted last.
pub(super) fn below_playing_video(doc: &Presentation, idx: usize, videos: &[ShapeId]) -> Vec<ShapeId> {
    let Some(slide) = doc.slides.get(idx) else { return vec![] };
    let Some(first) = slide.shapes.iter().position(|s| videos.contains(&s.id)) else { return vec![] };
    if slide.shapes.iter().skip(first + 1).all(|s| videos.contains(&s.id)) {
        return vec![];
    }
    slide.shapes.iter().enumerate().filter(|(i, s)| *i <= first || videos.contains(&s.id)).map(|(_, s)| s.id).collect()
}

/// Media on slide `idx` hidden until it plays, as it is when the slide is entered.
fn hidden_on_entry(doc: &Presentation, idx: usize) -> Vec<ShapeId> {
    crate::media::slide_media(doc, idx).into_iter().filter(|(_, _, clip)| clip.hide_while_not_playing).map(|m| m.0).collect()
}

/// Start rendering slide `next`'s first frame on another thread. `None` on the web (no threads).
pub(super) fn start_prefetch(doc: &std::sync::Arc<Presentation>, next: usize, size: (u32, u32)) -> Option<Prefetch> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let hidden = hidden_on_entry(doc, next);
    let key = Key { slide: next, step: 0, size, doc: std::sync::Arc::as_ptr(doc) as usize, hidden: hidden.clone(), gif: 0, plays: vec![] };
    let (tx, rx) = std::sync::mpsc::channel();
    let doc = doc.clone();
    // Leave a core for the UI thread.
    let threads = render_threads().saturating_sub(1).max(1);
    let spawned = std::thread::Builder::new().name("show-prefetch".into()).spawn(move || {
        let tl = ShowState::timeline(&doc, next);
        let _ = tx.send(render_frame(&doc, next, tl, 0, 0.0, size, hidden, &[], threads, &[], &[], None));
    });
    spawned.ok().map(|_| Prefetch { key, rx })
}

pub(super) fn fit(avail: Rect, doc: &Presentation) -> Rect {
    // Loaded sizes are repaired, but a zero side here would make every rectangle NaN.
    let side = |v: f64| if v.is_finite() { (v as f32).max(1.0) } else { 1.0 };
    let (sw, sh) = (side(doc.slide_size.width), side(doc.slide_size.height));
    let s = (avail.width() / sw).min(avail.height() / sh).max(0.0);
    Rect::from_center_size(avail.center(), vec2(sw * s, sh * s))
}

/// A transition layer as an egui mesh: [`deckcraft_anim::layer_mesh`]'s triangles (perspective
/// cards subdivided so their pictures stay straight) with per-vertex opacity.
/// Where presenter view's "Next" preview goes: the column's width below its label, kept inside
/// the top 60% of the column. A portrait slide used to get the full width and a height many
/// times the window (a white slab over the notes and past the window edge).
pub(super) fn next_rect(right: Rect, doc: &Presentation) -> Rect {
    let area = Rect::from_min_max(right.min + vec2(0.0, 22.0), pos2(right.max.x, right.min.y + 22.0 + (right.height() - 22.0).max(0.0) * 0.6));
    Rect::from_min_size(area.min, fit(area, doc).size())
}

pub(super) fn quad_mesh(tex: Option<egui::TextureId>, rect: Rect, l: &Layer, color: Color32) -> Mesh {
    let mut m = match tex {
        Some(id) => Mesh::with_texture(id),
        None => Mesh::default(),
    };
    let (verts, tris) = deckcraft_anim::layer_mesh(l);
    for v in &verts {
        let a = if v.a.is_finite() { v.a.clamp(0.0, 1.0) } else { 0.0 };
        let col = if tex.is_some() { Color32::from_white_alpha((a * 255.0).round() as u8) } else { color.gamma_multiply(a as f32) };
        let p = pos2(rect.min.x + v.pos[0] as f32 * rect.width(), rect.min.y + v.pos[1] as f32 * rect.height());
        m.vertices.push(egui::epaint::Vertex { pos: p, uv: pos2(v.uv[0] as f32, v.uv[1] as f32), color: col });
    }
    for t in tris {
        m.add_triangle(t[0], t[1], t[2]);
    }
    m
}

/// Render the backgrounds and contents of a transition's two slides into textures, once.
pub(super) fn part_textures(ctx: &egui::Context, doc: &Presentation, from: Option<usize>, to: usize, size: (u32, u32)) -> [Option<TextureHandle>; 4] {
    use deckcraft_render::SlidePart;
    let threads = if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) };
    let opts = RenderOpts { scale: size.0 as f64 / doc.slide_size.width.max(1.0), size: Some(size), threads, ..Default::default() };
    let tex = |i: usize, part: SlidePart, name: &str| {
        let img = deckcraft_render::render_slide_part(doc, i, part, &opts);
        crate::textures::upload(ctx, name, crate::textures::to_color_image(&img, false))
    };
    [
        from.map(|i| tex(i, SlidePart::Backdrop, "trans-old-back")),
        from.map(|i| tex(i, SlidePart::Content, "trans-old-content")),
        Some(tex(to, SlidePart::Backdrop, "trans-new-back")),
        Some(tex(to, SlidePart::Content, "trans-new-content")),
    ]
}

pub(super) fn render_thumb(ctx: &egui::Context, show: &mut Show, doc: &Presentation, i: usize) -> Option<TextureHandle> {
    if show.thumbs.len() >= GRID_CAP {
        return None;
    }
    let w = 320.0_f64;
    let h = (w * doc.slide_size.height / doc.slide_size.width.max(1.0)).round().max(1.0);
    let img = deckcraft_render::render_slide(
        doc,
        i,
        &RenderOpts { scale: w / doc.slide_size.width.max(1.0), size: Some((w as u32, h as u32)), ..Default::default() },
    );
    let tex = crate::textures::upload(ctx, format!("show-grid-{i}"), crate::textures::to_color_image(&img, false));
    show.thumbs.push((i, tex.clone()));
    Some(tex)
}
