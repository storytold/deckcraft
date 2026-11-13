//! Picture Format ▸ Artistic Effects: a picture redrawn as a pencil sketch, blurred, grainy…
//!
//! The effect is applied to the pixels and the result becomes the picture, as PowerPoint does;
//! the original is kept (`PictureFill::artistic`) so the effect can be changed or removed, and
//! both are saved (`a14:imgLayer`). The effects approximate PowerPoint's look.

use deckcraft_model::style::Artistic;
use deckcraft_model::{MediaId, ShapeId, ShapeKind};
use image::{GrayImage, Rgba, RgbaImage};
use serde_json::{Value, json};

use super::{CommandSpec, NO_ID_SPACE, bad, cmd, has_selection, str_param, targets};
use crate::{Result, Session};

/// The effects offered: `a14` element name and label.
pub const EFFECTS: &[(&str, &str)] = &[
    ("artisticBlur", "Blur"),
    ("artisticPencilGrayscale", "Pencil Grayscale"),
    ("artisticLineDrawing", "Line Drawing"),
    ("artisticFilmGrain", "Film Grain"),
    ("artisticCutout", "Cutout"),
    ("artisticGlowEdges", "Glow Edges"),
    ("artisticPhotocopy", "Photocopy"),
];

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "picture.artisticEffect",
        "Artistic Effects",
        ["Picture Format", "Adjust"],
        None,
        "{effect: none|artisticBlur|artisticPencilGrayscale|artisticLineDrawing|artisticFilmGrain|artisticCutout|artisticGlowEdges|artisticPhotocopy, ids?}",
        has_selection,
        artistic_effect
    )]
}

/// Largest side processed; bigger pictures are scaled down first.
const MAX_SIDE: u32 = 3000;

fn luma(p: &Rgba<u8>) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

fn gray(img: &RgbaImage) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| image::Luma([luma(img.get_pixel(x, y)).round().clamp(0.0, 255.0) as u8]))
}

/// Sobel edge strength, 0–255.
fn edges(g: &GrayImage) -> GrayImage {
    let (w, h) = g.dimensions();
    let at = |x: i64, y: i64| g.get_pixel(x.clamp(0, w as i64 - 1) as u32, y.clamp(0, h as i64 - 1) as u32)[0] as f32;
    GrayImage::from_fn(w, h, |x, y| {
        let (x, y) = (x as i64, y as i64);
        let gx = at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1) - at(x - 1, y - 1) - 2.0 * at(x - 1, y) - at(x - 1, y + 1);
        let gy = at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1) - at(x - 1, y - 1) - 2.0 * at(x, y - 1) - at(x + 1, y - 1);
        image::Luma([((gx * gx + gy * gy).sqrt() / 4.0).clamp(0.0, 255.0) as u8])
    })
}

/// Applies `effect` to `img` (alpha kept).
pub fn apply(effect: &str, img: &RgbaImage) -> Option<RgbaImage> {
    let (w, h) = img.dimensions();
    let scale = ((w * w + h * h) as f32).sqrt() / 1000.0;
    let alpha = |x: u32, y: u32| img.get_pixel(x, y)[3];
    let out = match effect {
        "artisticBlur" => image::imageops::blur(img, (6.0 * scale).max(1.0)),
        "artisticPencilGrayscale" | "artisticLineDrawing" | "artisticPhotocopy" => {
            let soft = image::imageops::blur(&gray(img), (0.8 * scale).max(0.5));
            let e = edges(&soft);
            RgbaImage::from_fn(w, h, |x, y| {
                let ev = e.get_pixel(x, y)[0] as f32;
                let gv = soft.get_pixel(x, y)[0] as f32;
                let v = match effect {
                    "artisticPencilGrayscale" => 255.0 - ev * 2.0 - (255.0 - gv) * 0.25,
                    "artisticLineDrawing" => {
                        if ev > 28.0 {
                            255.0 - (ev - 28.0) * 3.0
                        } else {
                            255.0
                        }
                    }
                    _ => {
                        if ev > 40.0 || gv < 70.0 {
                            0.0
                        } else {
                            255.0
                        }
                    }
                };
                let v = v.clamp(0.0, 255.0) as u8;
                Rgba([v, v, v, alpha(x, y)])
            })
        }
        "artisticFilmGrain" => {
            // Deterministic noise, so the same picture gives the same grain.
            let mut seed: u32 = 0x9E37_79B9;
            RgbaImage::from_fn(w, h, |x, y| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let n = (seed % 61) as f32 - 30.0;
                let p = img.get_pixel(x, y);
                let c = |v: u8| ((v as f32 - 128.0) * 1.1 + 128.0 + n).clamp(0.0, 255.0) as u8;
                Rgba([c(p[0]), c(p[1]), c(p[2]), p[3]])
            })
        }
        "artisticCutout" => {
            let soft = image::imageops::blur(img, (1.5 * scale).max(0.5));
            RgbaImage::from_fn(w, h, |x, y| {
                let p = soft.get_pixel(x, y);
                let q = |v: u8| ((v as f32 / 255.0 * 3.0).round() / 3.0 * 255.0) as u8;
                Rgba([q(p[0]), q(p[1]), q(p[2]), alpha(x, y)])
            })
        }
        "artisticGlowEdges" => {
            let e = edges(&gray(img));
            RgbaImage::from_fn(w, h, |x, y| {
                let k = (e.get_pixel(x, y)[0] as f32 / 255.0 * 2.5).min(1.0);
                let p = img.get_pixel(x, y);
                let c = |v: u8| (v as f32 * k + 255.0 * k * 0.3).clamp(0.0, 255.0) as u8;
                Rgba([c(p[0]), c(p[1]), c(p[2]), p[3]])
            })
        }
        _ => return None,
    };
    Some(out)
}

/// Decodes `data`, applies `effect` and encodes a PNG.
fn process(effect: &str, data: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(data).map_err(|_| bad("picture.artisticEffect", "this picture's format can't be processed"))?;
    let img = if img.width().max(img.height()) > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    let out = apply(effect, &img.to_rgba8()).ok_or_else(|| bad("picture.artisticEffect", "unknown effect"))?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(out)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|_| bad("picture.artisticEffect", "the processed picture couldn't be stored"))?;
    Ok(png)
}

fn artistic_effect(s: &mut Session, p: &Value) -> Result<Value> {
    let effect = match str_param(p, "effect") {
        Some("none") => None,
        Some(e) if EFFECTS.iter().any(|(n, _)| *n == e) => Some(e.to_string()),
        _ => return Err(bad("picture.artisticEffect", "`effect` is none or one of artisticBlur, artisticPencilGrayscale, …")),
    };
    let ids = targets(s, p)?;
    let st = s.doc()?;
    // (shape, original picture, its bytes and name)
    let mut work: Vec<(ShapeId, MediaId, Option<(Vec<u8>, String)>)> = Vec::new();
    for id in ids {
        let Some(ShapeKind::Picture { fill }) = st.shape(id).map(|sh| &sh.kind) else { continue };
        let original = fill.artistic.as_ref().map_or(fill.media, |a| a.original);
        let item = st.doc.media(original).map(|m| (m.data.to_vec(), m.name.clone()));
        work.push((id, original, item));
    }
    if work.is_empty() {
        return Err(bad("picture.artisticEffect", "select a picture"));
    }
    let mut results: Vec<(ShapeId, MediaId, Option<(String, Vec<u8>)>)> = Vec::new();
    for (id, original, item) in work {
        let processed = match (&effect, item) {
            (None, _) => None,
            (Some(e), Some((data, name))) => {
                Some((format!("{}-{}.png", name.rsplit_once('.').map_or(name.as_str(), |x| x.0), e), process(e, &data)?))
            }
            (Some(_), None) => return Err(bad("picture.artisticEffect", "the picture's data is missing")),
        };
        results.push((id, original, processed));
    }
    let effect2 = effect.clone();
    s.edit(move |doc, sel| {
        let mut set: Vec<(ShapeId, MediaId, Option<MediaId>)> = Vec::new();
        for (id, original, processed) in &results {
            let m = match processed {
                Some((name, png)) => Some(doc.add_media(name, "image/png", png.clone()).ok_or_else(|| bad("picture.artisticEffect", NO_ID_SPACE))?),
                None => None,
            };
            set.push((*id, *original, m));
        }
        let list = crate::shapes_mut(doc, sel).ok_or_else(|| bad("picture.artisticEffect", "no slide"))?;
        for (id, original, m) in set {
            if let Some(sh) = deckcraft_model::find_shape_mut(list, id)
                && let ShapeKind::Picture { fill } = &mut sh.kind
            {
                match (m, &effect2) {
                    (Some(m), Some(e)) => {
                        fill.media = m;
                        fill.artistic = Some(Artistic { effect: e.clone(), original });
                    }
                    _ => {
                        fill.media = original;
                        fill.artistic = None;
                    }
                }
            }
        }
        Ok(())
    })?;
    Ok(json!({"effect": effect}))
}
