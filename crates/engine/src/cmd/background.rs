//! Picture Format ▸ Remove Background: a classical matte, no model. The background is what the
//! picture's edges reach through similar colours: a flood fill from every edge pixel, each step
//! close to the pixel before it (gradients and soft shadows) and to the edge colour it started
//! from (so it can't leak into the subject). It turns transparent, with a one-pixel soft edge.

use std::collections::VecDeque;

use deckcraft_model::{ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Larger pictures are refused rather than matted slowly (16 megapixels).
const MAX_PIXELS: u64 = 16_000_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "picture.removeBackground",
        "Remove Background",
        ["Picture Format", "Adjust"],
        None,
        "{tolerance?: 0..100 (how different a colour may be from the edge's, default 20), ids?}",
        has_selection,
        remove_background
    )]
}

fn dist(a: [u8; 4], b: [u8; 4]) -> f64 {
    let d = |i: usize| f64::from(a[i]) - f64::from(b[i]);
    (d(0) * d(0) + d(1) * d(1) + d(2) * d(2)).sqrt()
}

/// Make the background of `img` transparent. `tolerance` is 0–100. Returns how many pixels
/// became transparent.
pub fn matte(img: &mut image::RgbaImage, tolerance: f64) -> usize {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w == 0 || h == 0 {
        return 0;
    }
    // Distances run 0–441 (black to white); the step limit follows gradients, the seed limit
    // keeps the fill on the edge's colour.
    let seed_tol = tolerance.clamp(0.0, 100.0) * 4.41;
    let step_tol = (seed_tol * 0.35).max(6.0);
    let px = |img: &image::RgbaImage, i: usize| img.get_pixel((i % w) as u32, (i / w) as u32).0;
    // 0: subject, 1: background; the seed colour each background pixel was reached from.
    let mut bg = vec![false; w * h];
    let mut seed = vec![[0u8; 4]; w * h];
    let mut queue = VecDeque::new();
    let push_edge = |i: usize, bg: &mut [bool], seed: &mut [[u8; 4]], q: &mut VecDeque<usize>| {
        if let Some(b) = bg.get_mut(i)
            && !*b
        {
            *b = true;
            if let Some(s) = seed.get_mut(i) {
                *s = px(img, i);
            }
            q.push_back(i);
        }
    };
    for x in 0..w {
        push_edge(x, &mut bg, &mut seed, &mut queue);
        push_edge((h - 1) * w + x, &mut bg, &mut seed, &mut queue);
    }
    for y in 0..h {
        push_edge(y * w, &mut bg, &mut seed, &mut queue);
        push_edge(y * w + w - 1, &mut bg, &mut seed, &mut queue);
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i % w, i / w);
        let here = px(img, i);
        let from = seed.get(i).copied().unwrap_or(here);
        let mut near = [None; 4];
        if x > 0 {
            near[0] = Some(i - 1);
        }
        if x + 1 < w {
            near[1] = Some(i + 1);
        }
        if y > 0 {
            near[2] = Some(i - w);
        }
        if y + 1 < h {
            near[3] = Some(i + w);
        }
        for j in near.into_iter().flatten() {
            if bg.get(j).copied().unwrap_or(true) {
                continue;
            }
            let c = px(img, j);
            // Transparent pixels are background already.
            if c[3] < 16 || (dist(c, here) <= step_tol && dist(c, from) <= seed_tol) {
                if let Some(b) = bg.get_mut(j) {
                    *b = true;
                }
                if let Some(s) = seed.get_mut(j) {
                    *s = from;
                }
                queue.push_back(j);
            }
        }
    }
    // A background edge that is the subject's colour throughout (the subject touches the
    // edge on every side) is no background: leave such a picture alone.
    let n = bg.iter().filter(|b| **b).count();
    if n == w * h {
        return 0;
    }
    let is_bg = |x: usize, y: usize| bg.get(y * w + x).copied().unwrap_or(false);
    let mut soft = vec![];
    for y in 0..h {
        for x in 0..w {
            if is_bg(x, y) {
                continue;
            }
            let touching =
                (x > 0 && is_bg(x - 1, y)) || (x + 1 < w && is_bg(x + 1, y)) || (y > 0 && is_bg(x, y - 1)) || (y + 1 < h && is_bg(x, y + 1));
            if touching {
                soft.push((x, y));
            }
        }
    }
    for (i, b) in bg.iter().enumerate() {
        if *b {
            img.get_pixel_mut((i % w) as u32, (i / w) as u32).0[3] = 0;
        }
    }
    for (x, y) in soft {
        let p = img.get_pixel_mut(x as u32, y as u32);
        p.0[3] = (u16::from(p.0[3]) * 2 / 3) as u8;
    }
    n
}

fn remove_background(s: &mut Session, p: &Value) -> Result<Value> {
    let tolerance = f64_or(p, "tolerance", 20.0);
    let ids = targets(s, p)?;
    let mut done: Vec<(ShapeId, String, Vec<u8>)> = vec![];
    {
        let st = s.doc()?;
        for id in &ids {
            let Some(sh) = st.shape(*id) else { continue };
            let ShapeKind::Picture { fill } = &sh.kind else { continue };
            let Some(item) = st.doc.media(fill.media) else { continue };
            let Ok(reader) = image::ImageReader::new(std::io::Cursor::new(item.data.as_slice())).with_guessed_format() else { continue };
            let Ok((iw, ih)) = reader.into_dimensions() else { continue };
            if u64::from(iw) * u64::from(ih) > MAX_PIXELS {
                return Err(bad("picture.removeBackground", format!("{iw}×{ih} pixels is too large; compress the picture first")));
            }
            let Ok(dynimg) = image::load_from_memory(&item.data) else { continue };
            let mut img = dynimg.to_rgba8();
            if matte(&mut img, tolerance) == 0 {
                continue;
            }
            let mut png = vec![];
            if image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_err() {
                continue;
            }
            let stem = item.name.rsplit_once('.').map_or(item.name.as_str(), |(a, _)| a);
            done.push((*id, format!("{stem}-no-background.png"), png));
        }
    }
    if done.is_empty() {
        return Err(bad("picture.removeBackground", "no background found: select a picture whose edges show its background"));
    }
    let n = done.len();
    s.edit(|doc, sel| {
        let added: Vec<(ShapeId, deckcraft_model::MediaId)> = done
            .into_iter()
            .map(|(id, name, png)| Ok((id, doc.add_media(&name, "image/png", png).ok_or_else(|| bad("picture.removeBackground", NO_ID_SPACE))?)))
            .collect::<Result<_>>()?;
        let shapes = crate::shapes_mut(doc, sel).ok_or_else(|| bad("picture.removeBackground", "no slide"))?;
        for (id, media) in added {
            if let Some(sh) = deckcraft_model::find_shape_mut(shapes, id)
                && let ShapeKind::Picture { fill } = &mut sh.kind
            {
                fill.media = media;
            }
        }
        Ok(())
    })?;
    Ok(json!({"pictures": n}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A red disc on a white-to-grey gradient.
    fn disc() -> image::RgbaImage {
        image::RgbaImage::from_fn(80, 60, |x, y| {
            let (dx, dy) = (f64::from(x) - 40.0, f64::from(y) - 30.0);
            if dx * dx + dy * dy < 15.0 * 15.0 {
                image::Rgba([220, 30, 30, 255])
            } else {
                let g = 255 - (x / 2) as u8;
                image::Rgba([g, g, g, 255])
            }
        })
    }

    #[test]
    fn matte_clears_the_gradient_and_keeps_the_subject() {
        let mut img = disc();
        let n = matte(&mut img, 20.0);
        assert!(n > 80 * 60 / 2, "{n}");
        assert_eq!(img.get_pixel(0, 0).0[3], 0);
        assert_eq!(img.get_pixel(79, 59).0[3], 0, "the darker end of the gradient");
        assert_eq!(img.get_pixel(40, 30).0, [220, 30, 30, 255]);
        // The disc's rim is softened, not cut.
        let rim = img.get_pixel(40, 16).0[3];
        assert!(rim > 0 && rim < 255 || img.get_pixel(40, 15).0[3] < 255, "{rim}");
        // A picture with nothing but subject at its edges is left alone.
        let mut flat = image::RgbaImage::from_pixel(10, 10, image::Rgba([10, 200, 10, 255]));
        assert_eq!(matte(&mut flat, 20.0), 0);
        assert!(flat.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn remove_background_replaces_the_picture_and_undoes() {
        let mut png = vec![];
        image::DynamicImage::ImageRgba8(disc()).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut s = Session::with_new();
        let r = s.execute("insert.picture", &json!({"data": base64_encode(&png), "name": "disc.png"})).unwrap();
        let id = r["id"].as_u64().unwrap();
        let media_of = |s: &Session| match &s.doc().unwrap().shape(ShapeId(id as u32)).unwrap().kind {
            ShapeKind::Picture { fill } => fill.media,
            _ => panic!("not a picture"),
        };
        let before = media_of(&s);
        s.execute("picture.removeBackground", &json!({"ids": [id]})).unwrap();
        let after = media_of(&s);
        assert_ne!(before, after);
        let item = s.doc().unwrap().doc.media(after).unwrap().clone();
        assert_eq!(item.name, "disc-no-background.png");
        let img = image::load_from_memory(&item.data).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(0, 0).0[3], 0);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(media_of(&s), before);
        // Not a picture: refused.
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [0, 0, 50, 50]})).unwrap();
        assert!(s.execute("picture.removeBackground", &json!({})).is_err());
    }
}
