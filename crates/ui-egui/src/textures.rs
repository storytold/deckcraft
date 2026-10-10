//! Rendered slides as GPU textures: the canvas slide and thumbnails, cached by the slide's and its
//! master's `Arc` identity (an unchanged slide keeps its `Arc`, so it never re-renders).

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_model::{Master, Presentation, ShapeId, Slide};
use deckcraft_render::{Image, RenderOpts};
use egui::{ColorImage, TextureHandle, TextureOptions};

#[derive(Clone, PartialEq)]
struct Key {
    slide: usize,
    master: usize,
    size: (u32, u32),
    edit: bool,
    language: crate::i18n::Language,
    extra: u64,
}

struct Entry {
    key: Key,
    tex: TextureHandle,
    // Keep the Arcs alive so their addresses can't be reused while cached.
    _slide: Arc<Slide>,
    _master: Option<Arc<Master>>,
    stamp: u64,
}

#[derive(Default)]
pub struct Textures {
    canvas: Option<Entry>,
    thumbs: HashMap<usize, Entry>,
    clock: u64,
    pub last_render_ms: f64,
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

fn master_of(p: &Presentation, slide: &Slide) -> Option<Arc<Master>> {
    p.masters.iter().find(|m| m.layout(slide.layout).is_some()).cloned()
}

fn threads() -> u16 {
    if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) }
}

impl Textures {
    /// The canvas texture for slide `index` at `size` pixels; `extra` changes with the GIF frames.
    pub fn slide(
        &mut self,
        ctx: &egui::Context,
        p: &Presentation,
        index: usize,
        size: (u32, u32),
        edit: bool,
        extra: u64,
        grayscale: bool,
        gif_times: &[(ShapeId, f64)],
    ) -> Option<TextureHandle> {
        let slide = p.slides.get(index)?.clone();
        let master = master_of(p, &slide);
        let key = Key {
            slide: Arc::as_ptr(&slide) as usize,
            master: master.as_ref().map(|m| Arc::as_ptr(m) as usize).unwrap_or(0),
            size,
            edit,
            language: if edit { crate::i18n::current() } else { crate::i18n::Language::En },
            extra: extra ^ grayscale as u64,
        };
        if let Some(e) = &self.canvas
            && e.key == key
        {
            return Some(e.tex.clone());
        }
        let t0 = crate::now_ms();
        let scale = size.0 as f64 / p.slide_size.width.max(1.0);
        let opts = RenderOpts {
            scale,
            edit,
            gif_times,
            placeholder_prompt: Some(crate::i18n::placeholder_prompt),
            threads: threads(),
            size: Some(size),
            ..Default::default()
        };
        let img = deckcraft_render::render_slide(p, index, &opts);
        self.last_render_ms = crate::now_ms() - t0;
        let ci = to_color_image(&img, grayscale);
        let tex = match self.canvas.take() {
            Some(mut e) => {
                e.tex.set(ci, TextureOptions::LINEAR);
                e.tex
            }
            None => ctx.load_texture("slide-canvas", ci, TextureOptions::LINEAR),
        };
        self.canvas = Some(Entry { key, tex: tex.clone(), _slide: slide, _master: master, stamp: 0 });
        Some(tex)
    }

    /// Force the canvas to re-render next time (fonts loaded, theme changed in place…).
    pub fn invalidate(&mut self) {
        self.canvas = None;
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
        let scale = width_px as f64 / p.slide_size.width.max(1.0);
        let img = deckcraft_render::render_slide(p, index, &RenderOpts { scale, size: Some((width_px, h)), threads: 0, ..Default::default() });
        let tex = ctx.load_texture(format!("thumb-{index}"), to_color_image(&img, false), TextureOptions::LINEAR);
        // Drop the old entry for this slide address; entries for slides that no longer exist age out.
        self.thumbs.insert(key.slide, Entry { key, tex: tex.clone(), _slide: slide, _master: master, stamp: clock });
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
    use super::*;

    #[test]
    fn switching_interface_language_refreshes_cached_hints_and_preserves_user_content() {
        fn updated_canvas(ctx: &egui::Context, textures: &mut Textures, p: &Presentation, language: crate::i18n::Language) -> egui::ImageData {
            crate::i18n::set_current(language);
            let tex = textures.slide(ctx, p, 0, (480, 270), true, 0, false, &[]).unwrap();
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
