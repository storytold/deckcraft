//! Rendered slides as GPU textures: the canvas slide and thumbnails, cached by the slide's and its
//! master's `Arc` identity (an unchanged slide keeps its `Arc`, so it never re-renders).
//!
//! The canvas texture covers the slide plus whatever of the slide's drawing reaches off it and is
//! in view (PowerPoint's Normal view shows off-slide objects; thumbnails clip to the slide).

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_geom::Rect;
use deckcraft_model::{Master, Presentation, ShapeId, Slide};
use deckcraft_render::{Image, Region, RenderOpts};
use egui::{ColorImage, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode};

#[derive(Clone, PartialEq)]
struct Key {
    slide: usize,
    master: usize,
    size: (u32, u32),
    edit: bool,
    language: crate::i18n::Language,
    extra: u64,
    /// The pasteboard region the slide's drawing calls for (canvas only).
    want: Region,
    editing_text: Option<ShapeId>,
}

struct Entry {
    key: Key,
    tex: TextureHandle,
    /// The region the texture covers (at least `key.want`).
    region: Region,
    // Keep the Arcs alive so their addresses can't be reused while cached.
    _slide: Arc<Slide>,
    _master: Option<Arc<Master>>,
    stamp: u64,
}

#[derive(Default)]
pub struct Textures {
    canvas: Option<Entry>,
    /// How far the canvas slide's shapes reach (from their boxes), for the slide and master `Arc`s
    /// held (kept alive so their addresses can't be reused).
    estimate: Option<(Arc<Slide>, Option<Arc<Master>>, Rect)>,
    /// How far the last canvas render of a slide (by index) actually drew: text spilling out of
    /// its box shows there.
    drawn: Option<(usize, Rect)>,
    /// Canvas renders so far (tests, performance checks).
    pub canvas_renders: u64,
    thumbs: HashMap<usize, Entry>,
    /// Thumbnails actually rendered, over the life of this cache. The number *held*
    /// ([`Textures::thumb_count`]) cannot see a pane that renders the same rows over and over --
    /// the cache thrash behind #969 -- because the count settles while the work does not.
    thumb_renders: u64,
    /// SmartArt gallery pictures by layout, style and colours (and, for hover previews, the
    /// outline's hash).
    smartart: HashMap<String, TextureHandle>,
    clock: u64,
    pub last_render_ms: f64,
}

impl Textures {
    /// SmartArt gallery pictures cached so far (tests).
    pub fn smartart_cached(&self) -> usize {
        self.smartart.len()
    }

    /// A SmartArt layout drawn small for a gallery (`nodes` for a preview of the selected
    /// graphic's own outline), cached; drawn with the active presentation's theme.
    pub fn smartart(
        &mut self,
        ctx: &egui::Context,
        p: &deckcraft_model::Presentation,
        slide: usize,
        kind: &str,
        style: &str,
        colors: &str,
        nodes: Option<&[deckcraft_model::DiagramNode]>,
        width_px: u32,
    ) -> Option<TextureHandle> {
        let mut key = format!("{kind}|{style}|{colors}|{width_px}");
        if let Some(n) = nodes {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for x in n {
                (x.text.as_str(), x.level).hash(&mut h);
            }
            key.push_str(&format!("|{:x}", h.finish()));
        }
        if let Some(t) = self.smartart.get(&key) {
            return Some(t.clone());
        }
        let mut s = p.slides.get(slide).or_else(|| p.slides.first())?.as_ref().clone();
        let g = deckcraft_engine::cmd::smartart::preview(kind, style, colors, nodes);
        let id = g.id;
        s.shapes = vec![g];
        let img = deckcraft_render::render_shape(p, &s, id, width_px as f64 / 336.0)?;
        let tex = upload(ctx, format!("smartart-{key}"), to_color_image(&img, false));
        if self.smartart.len() > 300 {
            self.smartart.clear();
        }
        self.smartart.insert(key, tex.clone());
        Some(tex)
    }
}

/// Snap `rect` onto the physical pixel grid.
///
/// The editor draws the slide as a texture of device pixels. A rect that starts or ends
/// between pixels is sampled with a linear filter, so every glyph edge is a blend of two
/// texels and small text looks soft. On a Retina display that offset is half a point, which
/// is the whole edge of an editing-size glyph. The slide show draws much larger glyphs, so
/// the same blend is not visible there (#184).
pub fn device_pixel_rect(rect: egui::Rect, ppp: f32) -> egui::Rect {
    if !ppp.is_finite() || ppp <= 0.0 || !rect.is_finite() {
        return rect;
    }
    let snap = |v: f32| {
        let p = v * ppp;
        if p.is_finite() { p.round() / ppp } else { v }
    };
    let min = egui::pos2(snap(rect.min.x), snap(rect.min.y));
    let step = 1.0 / ppp;
    let max = egui::pos2(snap(rect.max.x).max(min.x + step), snap(rect.max.y).max(min.y + step));
    egui::Rect::from_min_max(min, max)
}

/// Editor slide textures. Nearest magnification keeps a texel that is stretched by a
/// fraction of a pixel from blurring the glyph; minification still blends when many texels
/// share a screen pixel.
pub fn editor_texture_options() -> TextureOptions {
    TextureOptions {
        magnification: TextureFilter::Nearest,
        minification: TextureFilter::Linear,
        wrap_mode: TextureWrapMode::ClampToEdge,
        mipmap_mode: None,
    }
}

pub fn to_color_image(img: &Image, grayscale: bool) -> ColorImage {
    let w = img.width as usize;
    let h = img.height as usize;
    let mut px = Vec::with_capacity(w * h);
    for c in img.pixels.as_chunks::<4>().0 {
        let (r, g, b, a) = (c[0], c[1], c[2], c[3]);
        if grayscale {
            let y = ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8;
            px.push(egui::Color32::from_rgba_premultiplied(y, y, y, a));
        } else {
            px.push(egui::Color32::from_rgba_premultiplied(r, g, b, a));
        }
    }
    px.resize(w * h, egui::Color32::TRANSPARENT);
    ColorImage::new([w, h], px)
}

/// The largest texture side the renderer (GPU or software fallback) takes; egui panics on
/// a bigger texture.
pub fn max_side(ctx: &egui::Context) -> u32 {
    ctx.input(|i| i.max_texture_side).clamp(1, u32::MAX as usize) as u32
}

/// `(w, h)` scaled down, keeping its shape, so neither side exceeds `max` (and none is 0).
/// Render sizes go through this so a huge display, DPI or slide shape doesn't render a
/// picture the GPU can't hold.
pub fn fit_size((w, h): (u32, u32), max: u32) -> (u32, u32) {
    let (w, h, max) = (w.max(1), h.max(1), max.max(1));
    let big = w.max(h);
    if big <= max {
        return (w, h);
    }
    let k = max as f64 / big as f64;
    (((w as f64 * k).floor() as u32).clamp(1, max), ((h as f64 * k).floor() as u32).clamp(1, max))
}

/// `img` shrunk by whole-pixel box averaging until both sides fit `max`.
pub fn shrink_to(img: ColorImage, max: u32) -> ColorImage {
    let [w, h] = img.size;
    let max = max.max(1) as usize;
    if w <= max && h <= max {
        return img;
    }
    let f = w.max(h).div_ceil(max);
    let (nw, nh) = (w.div_ceil(f).max(1), h.div_ceil(f).max(1));
    let mut px = Vec::with_capacity(nw * nh);
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for sy in y * f..((y + 1) * f).min(h) {
                for sx in x * f..((x + 1) * f).min(w) {
                    if let Some(c) = img.pixels.get(sy * w + sx) {
                        for (a, v) in acc.iter_mut().zip(c.to_array()) {
                            *a += v as u32;
                        }
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            px.push(egui::Color32::from_rgba_premultiplied((acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (acc[3] / n) as u8));
        }
    }
    ColorImage::new([nw, nh], px)
}

/// Upload a texture, shrunk to the renderer's limit if need be.
pub fn upload(ctx: &egui::Context, name: impl Into<String>, img: ColorImage) -> TextureHandle {
    ctx.load_texture(name, shrink_to(img, max_side(ctx)), TextureOptions::LINEAR)
}

/// Replace a texture's picture, shrunk to the renderer's limit if need be.
pub fn replace(ctx: &egui::Context, tex: &mut TextureHandle, img: ColorImage) {
    tex.set(shrink_to(img, max_side(ctx)), TextureOptions::LINEAR);
}

/// [`upload`] for editor slide pictures, drawn on the device pixel grid with
/// [`editor_texture_options`] (#184).
pub fn upload_editor(ctx: &egui::Context, name: impl Into<String>, img: ColorImage) -> TextureHandle {
    ctx.load_texture(name, shrink_to(img, max_side(ctx)), editor_texture_options())
}

/// [`replace`] for editor slide pictures (#184).
pub fn replace_editor(ctx: &egui::Context, tex: &mut TextureHandle, img: ColorImage) {
    tex.set(shrink_to(img, max_side(ctx)), editor_texture_options());
}

fn master_of(p: &Presentation, slide: &Slide) -> Option<Arc<Master>> {
    p.masters.iter().find(|m| m.layout(slide.layout).is_some()).cloned()
}

fn same(a: &Option<Arc<Master>>, b: &Option<Arc<Master>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Mix the preview playhead into the canvas cache key. Zero means "no preview", so a playhead of
/// 0s still misses the at-rest texture (entrances start hidden).
fn anim_key(anim: Option<(&deckcraft_anim::Timeline, f64)>) -> u64 {
    let Some((_, t)) = anim else { return 0 };
    let ms = if t.is_finite() { (t * 1000.0).clamp(0.0, 1.0e12) as u64 } else { 0 };
    ms.wrapping_add(1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn threads() -> u16 {
    if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) }
}

impl Textures {
    /// The canvas texture for slide `index`, the slide being `size` pixels, and the region it
    /// covers: the slide, grown over the pasteboard to show what reaches off the slide within
    /// `visible` (the canvas, in pixels from the slide's top-left corner). `extra` changes with the
    /// GIF frames. `editing_text` replaces its placeholder prompt with gray empty list markers.
    #[allow(clippy::too_many_arguments)]
    pub fn slide(
        &mut self,
        ctx: &egui::Context,
        p: &Presentation,
        index: usize,
        size: (u32, u32),
        visible: Region,
        edit: bool,
        extra: u64,
        grayscale: bool,
        gif_times: &[(ShapeId, f64)],
        editing_text: Option<ShapeId>,
        anim: Option<(&deckcraft_anim::Timeline, f64)>,
    ) -> Option<(TextureHandle, Region)> {
        let slide = p.slides.get(index)?.clone();
        let master = master_of(p, &slide);
        let scale = size.0 as f64 / p.slide_size.width.max(1.0);
        let estimate = match &self.estimate {
            Some((s, m, r)) if Arc::ptr_eq(s, &slide) && same(m, &master) => *r,
            _ => {
                let r = deckcraft_render::slide_extent(p, index);
                self.estimate = Some((slide.clone(), master.clone(), r));
                r
            }
        };
        let reach = match self.drawn {
            Some((i, d)) if i == index => estimate.union(d),
            _ => estimate,
        };
        // The GPU's (or software fallback's) texture limit: a larger canvas texture panics in egui.
        let max_side = max_side(ctx);
        let want = fit_texture(deckcraft_render::pasteboard_region(size, scale, reach, visible), visible, max_side);
        let key = Key {
            slide: Arc::as_ptr(&slide) as usize,
            master: master.as_ref().map(|m| Arc::as_ptr(m) as usize).unwrap_or(0),
            size,
            edit,
            language: if edit { crate::i18n::current() } else { crate::i18n::Language::En },
            extra: extra ^ grayscale as u64 ^ anim_key(anim),
            want,
            editing_text,
        };
        if let Some(e) = &self.canvas
            && e.key == key
        {
            return Some((e.tex.clone(), e.region));
        }
        let t0 = crate::now_ms();
        let render = |region: Region| {
            let state_of = |id: ShapeId| anim.map(|(tl, time)| crate::preview::shape_state_at(tl, id, time)).unwrap_or_default();
            let state: Option<&dyn Fn(ShapeId) -> deckcraft_render::ShapeState> = if anim.is_some() { Some(&state_of) } else { None };
            let opts = RenderOpts {
                scale,
                edit,
                gif_times,
                placeholder_prompt: Some(crate::i18n::placeholder_prompt),
                threads: threads(),
                offset: region.offset(),
                size: Some(region.size()),
                state,
                editing_text,
                ..Default::default()
            };
            deckcraft_render::render_slide_drawn(p, index, &opts)
        };
        let (mut img, drawn) = render(want);
        self.canvas_renders += 1;
        let mut region = want;
        // Text spilling out of its box can reach further than the boxes said. Once drawn, the
        // region it calls for is known (and is next frame's key): render again only if it shows
        // more than this render covered.
        let key =
            Key { want: fit_texture(deckcraft_render::pasteboard_region(size, scale, estimate.union(drawn), visible), visible, max_side), ..key };
        if !region.contains(&key.want) {
            region = key.want;
            img = render(region).0;
            self.canvas_renders += 1;
        }
        self.drawn = Some((index, drawn));
        self.last_render_ms = crate::now_ms() - t0;
        let ci = to_color_image(&img, grayscale);
        let tex = match self.canvas.take() {
            Some(mut e) => {
                replace_editor(ctx, &mut e.tex, ci);
                e.tex
            }
            None => upload_editor(ctx, "slide-canvas", ci),
        };
        self.canvas = Some(Entry { key, tex: tex.clone(), region, _slide: slide, _master: master, stamp: 0 });
        Some((tex, region))
    }

    /// Cached thumbnails (bounded; soak tests check it).
    pub fn thumb_count(&self) -> usize {
        self.thumbs.len()
    }

    /// How many thumbnails this cache has rendered, ever. A pane that is done asks for no more:
    /// the number stops climbing, which is what tells a thrashing pane from a settled one.
    pub fn thumb_renders(&self) -> u64 {
        self.thumb_renders
    }

    /// Force the canvas to re-render next time (fonts loaded, theme changed in place…).
    pub fn invalidate(&mut self) {
        self.canvas = None;
        self.estimate = None;
        self.drawn = None;
        self.thumbs.clear();
    }

    /// A thumbnail texture; renders at most `budget` new thumbnails per frame (returns None when
    /// it is over budget, so the caller draws a placeholder and asks for another frame).
    pub fn thumb(&mut self, ctx: &egui::Context, p: &Presentation, index: usize, width_px: u32, budget: &mut u32) -> Option<TextureHandle> {
        let slide = p.slides.get(index)?.clone();
        let master = master_of(p, &slide);
        let h = (width_px as f64 * p.slide_size.height / p.slide_size.width.max(1.0)).round().max(1.0) as u32;
        let key = Key {
            slide: Arc::as_ptr(&slide) as usize,
            master: master.as_ref().map(|m| Arc::as_ptr(m) as usize).unwrap_or(0),
            size: (width_px, h),
            edit: false,
            language: crate::i18n::Language::En,
            extra: 0,
            want: Region::slide(width_px, h),
            editing_text: None,
        };
        self.clock += 1;
        let clock = self.clock;
        if let Some(e) = self.thumbs.get_mut(&key.slide)
            && e.key == key
        {
            e.stamp = clock;
            return Some(e.tex.clone());
        }
        if *budget == 0 {
            // Keep showing the stale thumbnail while the new one waits.
            return self.thumbs.get(&key.slide).map(|e| e.tex.clone());
        }
        *budget -= 1;
        let (tw, th) = fit_size((width_px, h), max_side(ctx));
        let scale = tw as f64 / p.slide_size.width.max(1.0);
        let img = deckcraft_render::render_slide(p, index, &RenderOpts { scale, size: Some((tw, th)), threads: 0, ..Default::default() });
        self.thumb_renders += 1;
        let tex = upload_editor(ctx, format!("thumb-{index}"), to_color_image(&img, false));
        // Drop the old entry for this slide address; entries for slides that no longer exist age out.
        let region = key.want;
        self.thumbs.insert(key.slide, Entry { key, tex: tex.clone(), region, _slide: slide, _master: master, stamp: clock });
        if self.thumbs.len() > 600 {
            let mut v: Vec<(usize, u64)> = self.thumbs.iter().map(|(k, e)| (*k, e.stamp)).collect();
            v.sort_by_key(|x| x.1);
            for (k, _) in v.into_iter().take(200) {
                self.thumbs.remove(&k);
            }
        }
        Some(tex)
    }
}

#[cfg(test)]
mod tests {
    use super::device_pixel_rect;
    use egui::{Rect, pos2, vec2};

    fn on_grid(v: f32, ppp: f32) -> bool {
        let px = v * ppp;
        (px - px.round()).abs() < 1e-3
    }

    #[test]
    fn a_retina_slide_rect_lands_on_device_pixels() {
        let ppp = 2.0;
        let r = Rect::from_min_size(pos2(10.3, 20.25), vec2(100.2, 50.4));
        let s = device_pixel_rect(r, ppp);
        assert!(on_grid(s.min.x, ppp) && on_grid(s.min.y, ppp) && on_grid(s.max.x, ppp) && on_grid(s.max.y, ppp));
        assert!(s.width() > 0.0 && s.height() > 0.0);
        // One texture pixel per device pixel: the snapped size is an integer pixel count.
        let px = ((s.width() * ppp).round() as u32, (s.height() * ppp).round() as u32);
        assert!((s.width() / px.0 as f32 * ppp - 1.0).abs() < 1e-3);
        assert!((s.height() / px.1 as f32 * ppp - 1.0).abs() < 1e-3);
        let aligned = Rect::from_min_max(pos2(1.0, 2.5), pos2(80.0, 40.0));
        assert_eq!(device_pixel_rect(aligned, ppp), aligned);
    }

    #[test]
    fn a_bad_scale_does_not_move_the_rect() {
        let r = Rect::from_min_size(pos2(3.0, 4.0), vec2(10.0, 12.0));
        assert_eq!(device_pixel_rect(r, 0.0), r);
        assert_eq!(device_pixel_rect(r, f32::NAN), r);
    }
}

/// `r` cut to at most `max` pixels a side, keeping as much of `visible` as fits.
pub(crate) fn fit_texture(mut r: Region, visible: Region, max: u32) -> Region {
    fn axis(start: &mut i32, len: &mut u32, vis: i32, max: u32) {
        if *len > max {
            let hi = *start as i64 + (*len - max) as i64;
            *start = (vis as i64).clamp(*start as i64, hi) as i32;
            *len = max;
        }
    }
    axis(&mut r.x, &mut r.w, visible.x, max);
    axis(&mut r.y, &mut r.h, visible.y, max);
    r
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    #[test]
    fn canvas_textures_stay_within_the_texture_limit() {
        let r = Region { x: -100, y: -50, w: 5000, h: 1000 };
        let f = fit_texture(r, Region { x: 300, y: 0, w: 800, h: 600 }, 2048);
        assert_eq!((f.x, f.y, f.w, f.h), (300, -50, 2048, 1000));
        // Visible past the far end: the cut is pinned there.
        let f = fit_texture(r, Region { x: 4800, y: 0, w: 800, h: 600 }, 2048);
        assert_eq!((f.x, f.w), (-100 + 5000 - 2048, 2048));
        assert_eq!(fit_texture(r, r, 8192), r);
    }

    #[test]
    fn render_sizes_and_pictures_shrink_to_the_limit_keeping_their_shape() {
        assert_eq!(fit_size((195, 10920), 2048), (36, 2048));
        assert_eq!(fit_size((3000, 1500), 2048), (2048, 1024));
        assert_eq!(fit_size((800, 600), 2048), (800, 600));
        assert_eq!(fit_size((0, 0), 0), (1, 1));
        // A 5000 × 3 picture: half red, half blue, averaged down to 1667 × 1.
        let img = ColorImage::new(
            [5000, 3],
            (0..3)
                .flat_map(|y| (0..5000).map(move |x| (x, y)))
                .map(|(x, _)| if x < 2500 { egui::Color32::RED } else { egui::Color32::BLUE })
                .collect(),
        );
        let small = shrink_to(img, 2048);
        assert_eq!(small.size, [1667, 1]);
        assert_eq!(small.pixels[0], egui::Color32::RED);
        assert_eq!(small.pixels[1666], egui::Color32::BLUE);
        let ok = ColorImage::filled([10, 10], egui::Color32::WHITE);
        assert_eq!(shrink_to(ok.clone(), 2048).size, ok.size);
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn switching_interface_language_refreshes_cached_hints_and_preserves_user_content() {
        fn updated_canvas(ctx: &egui::Context, textures: &mut Textures, p: &Presentation, language: crate::i18n::Language) -> egui::ImageData {
            crate::i18n::set_current(language);
            let (tex, _) = textures.slide(ctx, p, 0, (480, 270), Region::slide(480, 270), true, 0, false, &[], None, None).unwrap();
            let mut delta = ctx.tex_manager().write().take_delta();
            let image = delta.set.get(&tex.id()).and_then(|images| images.last()).map(|image| image.image.clone());
            delta.clear();
            image.expect("the changed interface language must update the canvas texture")
        }

        let ctx = egui::Context::default();
        let mut textures = Textures::default();
        let mut p = Presentation::default();
        let before = p.clone();
        let english = updated_canvas(&ctx, &mut textures, &p, crate::i18n::Language::En);
        let ukrainian = updated_canvas(&ctx, &mut textures, &p, crate::i18n::Language::Uk);
        assert!(english != ukrainian, "the default placeholder hints must change visibly");
        assert_eq!(p, before);

        for master in &mut p.masters {
            for layout in &mut Arc::make_mut(master).layouts {
                for shape in &mut layout.shapes {
                    if let Some(ph) = &mut shape.ph {
                        ph.has_custom_prompt = true;
                        shape.text = Some(deckcraft_model::TextBody::from_text("Custom {project} prompt"));
                    }
                }
            }
        }
        Arc::make_mut(p.slides.first_mut().unwrap()).shapes.first_mut().unwrap().text =
            Some(deckcraft_model::TextBody::from_text("User File {project}"));
        let before = p.clone();
        let english = updated_canvas(&ctx, &mut textures, &p, crate::i18n::Language::En);
        let ukrainian = updated_canvas(&ctx, &mut textures, &p, crate::i18n::Language::Uk);
        assert!(english == ukrainian, "stored text and custom prompts must retain their original wording");
        assert_eq!(p, before);
        crate::i18n::set_current(crate::i18n::Language::En);
    }
}
