//! Picture Format ▸ Adjust ▸ Compress Pictures: downsample pictures to a target resolution for
//! the size they are shown at, and optionally delete their cropped-away areas.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use deckcraft_model::style::PictureFill;
use deckcraft_model::{MediaId, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "picture.compress",
        "Compress Pictures",
        ["Picture Format", "Adjust"],
        None,
        "{ppi?: 330|220|150|96 (default 220), deleteCropped?: bool (default true), all?: bool (every picture in the presentation; default the selected pictures), ids?} → {pictures, before, after}",
        has_doc,
        compress
    )]
}

/// A picture compressed for one shape: new bytes, content type and the crop it now needs.
struct Packed {
    bytes: Vec<u8>,
    content_type: &'static str,
    crop: [f64; 4],
}

/// Whether `crop` cuts something away (and only cuts: negative values pad, and are kept).
fn real_crop(c: &[f64; 4]) -> bool {
    c.iter().all(|v| (0.0..1.0).contains(v)) && c.iter().any(|v| *v > 0.0) && c[0] + c[2] < 1.0 && c[1] + c[3] < 1.0
}

/// Largest picture decoded for compression, in pixels (a 61 MP camera frame fits); bigger ones
/// are left as they are.
const MAX_PIXELS: u64 = 64_000_000;
/// Most memory the decoder may allocate for one picture.
const MAX_DECODE_BYTES: u64 = 512 << 20;

/// Decode `data` within [`MAX_PIXELS`] and [`MAX_DECODE_BYTES`]: the size is read from the header
/// first, so a picture claiming to be huge is never decoded.
fn decode(data: &[u8], format: image::ImageFormat) -> Option<image::DynamicImage> {
    let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(data), format).into_dimensions().ok()?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(data), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(w);
    limits.max_image_height = Some(h);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    reader.decode().ok()
}

/// Compress `data` for a picture shown `w_pt` × `h_pt` with `crop`, at `ppi`. `None` when the
/// format isn't a raster we re-encode (SVG, EMF, GIF…) or nothing would get smaller.
fn pack(data: &[u8], crop: [f64; 4], w_pt: f64, h_pt: f64, ppi: f64, delete_cropped: bool) -> Option<Packed> {
    use image::ImageFormat as F;
    let format = image::guess_format(data).ok()?;
    if !matches!(format, F::Png | F::Jpeg | F::Bmp | F::Tiff | F::WebP) {
        return None;
    }
    let mut img = decode(data, format)?;
    let (iw, ih) = (img.width() as f64, img.height() as f64);
    if iw < 1.0 || ih < 1.0 {
        return None;
    }
    let cut = delete_cropped && real_crop(&crop);
    let mut new_crop = crop;
    // The share of the image that is visible across and down.
    let (mut fx, mut fy) = ((1.0 - crop[0] - crop[2]).max(0.01), (1.0 - crop[1] - crop[3]).max(0.01));
    if cut {
        let x = (crop[0] * iw).floor() as u32;
        let y = (crop[1] * ih).floor() as u32;
        let w = ((fx * iw).round() as u32).clamp(1, img.width() - x.min(img.width() - 1));
        let h = ((fy * ih).round() as u32).clamp(1, img.height() - y.min(img.height() - 1));
        img = img.crop_imm(x, y, w, h);
        new_crop = [0.0; 4];
        (fx, fy) = (1.0, 1.0);
    } else if !crop.iter().all(|v| v.is_finite()) {
        return None;
    }
    // Pixels needed for the whole (possibly still cropped) image at `ppi`.
    let need_w = (w_pt.abs() / 72.0 * ppi / fx).ceil().max(1.0);
    let need_h = (h_pt.abs() / 72.0 * ppi / fy).ceil().max(1.0);
    let k = (need_w / img.width() as f64).max(need_h / img.height() as f64);
    let resized = k < 1.0;
    if resized {
        let w = ((img.width() as f64 * k).round() as u32).max(1);
        let h = ((img.height() as f64 * k).round() as u32).max(1);
        img = img.resize_exact(w, h, image::imageops::FilterType::Lanczos3);
    }
    if !resized && !cut {
        return None;
    }
    let alpha = img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p.0[3] < 255);
    let mut out = std::io::Cursor::new(Vec::new());
    let content_type = if format == F::Jpeg && !alpha {
        let rgb = img.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&rgb).ok()?;
        "image/jpeg"
    } else {
        img.write_to(&mut out, F::Png).ok()?;
        "image/png"
    };
    let bytes = out.into_inner();
    // Never make a picture bigger just to resample it; deleting a crop is worth it regardless.
    if bytes.len() >= data.len() && !cut {
        return None;
    }
    Some(Packed { bytes, content_type, crop: new_crop })
}

/// Picture shapes in `shapes` (groups included) that `want` accepts.
fn pictures_mut<'a>(shapes: &'a mut [Shape], want: &dyn Fn(ShapeId) -> bool, out: &mut Vec<&'a mut Shape>, depth: usize) {
    if depth > 64 {
        return;
    }
    for sh in shapes {
        if matches!(sh.kind, ShapeKind::Picture { .. }) && want(sh.id) {
            out.push(sh);
        } else if let Some(ch) = sh.children_mut() {
            pictures_mut(ch, want, out, depth + 1);
        }
    }
}

fn compress(s: &mut Session, p: &Value) -> Result<Value> {
    let ppi = f64_param(p, "ppi").unwrap_or(220.0);
    if !ppi.is_finite() || !(36.0..=1200.0).contains(&ppi) {
        return Err(bad("picture.compress", "`ppi` must be between 36 and 1200 (PowerPoint offers 330, 220, 150 and 96)"));
    }
    let delete_cropped = bool_or(p, "deleteCropped", true);
    let all = bool_or(p, "all", false);
    let ids: HashSet<ShapeId> = if all { HashSet::new() } else { targets(s, p)?.into_iter().collect() };
    if !all && ids.is_empty() {
        return Err(bad("picture.compress", "select a picture, or pass all: true"));
    }
    let current = s.doc()?.selection.slide;
    s.edit(|doc, _| {
        let want = |id: ShapeId| all || ids.contains(&id);
        let mut before = 0usize;
        let mut after = 0usize;
        let mut count = 0usize;
        let mut changed: Vec<MediaId> = vec![];
        // The same picture at the same size and crop compresses once.
        let mut cache: HashMap<(u32, [u64; 4], u64, u64), Option<(MediaId, [f64; 4], usize)>> = HashMap::new();
        let range: Vec<usize> = if all { (0..doc.slides.len()).collect() } else { vec![current] };
        for i in range {
            let Some(slide) = doc.slides.get(i).cloned() else { continue };
            let mut slide = slide.as_ref().clone();
            let mut pics = vec![];
            pictures_mut(&mut slide.shapes, &want, &mut pics, 0);
            let mut touched = false;
            for sh in pics {
                let (w, h) = sh.xfrm.map(|x| (x.w, x.h)).unwrap_or((0.0, 0.0));
                let ShapeKind::Picture { fill } = &mut sh.kind else { continue };
                let PictureFill { media, crop, .. } = fill.clone();
                let key = (media.0, crop.map(f64::to_bits), w.to_bits(), h.to_bits());
                let hit = match cache.get(&key) {
                    Some(v) => *v,
                    None => {
                        let packed = doc.media(media).filter(|m| !m.data.is_empty()).and_then(|m| {
                            let old = m.data.len();
                            pack(&m.data, crop, w, h, ppi, delete_cropped).map(|pk| (pk, old, m.name.clone()))
                        });
                        let v = match packed {
                            Some((pk, old, name)) => {
                                before += old;
                                after += pk.bytes.len();
                                let stem = std::path::Path::new(&name).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or(name);
                                let ext = if pk.content_type == "image/jpeg" { "jpg" } else { "png" };
                                let id = doc
                                    .add_media(&format!("{stem}.{ext}"), pk.content_type, pk.bytes)
                                    .ok_or_else(|| bad("picture.compress", NO_ID_SPACE))?;
                                Some((id, pk.crop, old))
                            }
                            None => None,
                        };
                        cache.insert(key, v);
                        v
                    }
                };
                if let Some((id, new_crop, _)) = hit {
                    changed.push(media);
                    fill.media = id;
                    fill.crop = new_crop;
                    count += 1;
                    touched = true;
                }
            }
            if touched && let Some(sl) = doc.slides.get_mut(i) {
                *sl = Arc::new(slide);
            }
        }
        // Drop the originals nothing uses any more, so the saved file actually shrinks.
        if !changed.is_empty() {
            // Typed walk of every reference (artistic originals, fills, posters, sounds, fonts…).
            let used: HashSet<MediaId> = crate::media_refs::presentation_media(doc).into_iter().collect();
            doc.media.retain(|m| !changed.contains(&m.id) || used.contains(&m.id));
        }
        Ok(json!({"pictures": count, "before": before, "after": after, "ppi": ppi}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7 % 256) as u8, (y * 5 % 256) as u8, ((x ^ y) % 256) as u8]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn pack_downsamples_to_the_shown_size() {
        // 2000 px shown at 1 inch: 150 ppi needs 150 px.
        let pk = pack(&png(2000, 1000), [0.0; 4], 72.0, 36.0, 150.0, true).unwrap();
        let img = image::load_from_memory(&pk.bytes).unwrap();
        assert_eq!((img.width(), img.height()), (150, 75));
        assert_eq!(pk.content_type, "image/png");
        // Already small enough: nothing to do.
        assert!(pack(&png(100, 50), [0.0; 4], 72.0, 36.0, 150.0, true).is_none());
        // Not a raster we re-encode.
        assert!(pack(b"<svg xmlns='http://www.w3.org/2000/svg'/>", [0.0; 4], 72.0, 72.0, 150.0, true).is_none());
    }

    #[test]
    fn pack_deletes_cropped_areas_or_keeps_them() {
        let src = png(400, 400);
        // Left half cropped away, shown 2 in wide at 100 ppi → 200 px of the visible half.
        let pk = pack(&src, [0.5, 0.0, 0.0, 0.0], 144.0, 288.0, 100.0, true).unwrap();
        assert_eq!(pk.crop, [0.0; 4]);
        let img = image::load_from_memory(&pk.bytes).unwrap();
        assert_eq!((img.width(), img.height()), (200, 400));
        // Keeping the crop: the whole image stays, scaled so the visible half has its pixels.
        let pk = pack(&src, [0.5, 0.0, 0.0, 0.0], 72.0, 72.0, 100.0, false).unwrap();
        assert_eq!(pk.crop, [0.5, 0.0, 0.0, 0.0]);
        let img = image::load_from_memory(&pk.bytes).unwrap();
        assert_eq!(img.width(), 200);
    }

    /// A PNG whose header claims `w` × `h` (no pixel data): enough for the size check.
    fn png_header(w: u32, h: u32) -> Vec<u8> {
        fn crc(bytes: &[u8]) -> u32 {
            let mut c = 0xFFFF_FFFFu32;
            for b in bytes {
                c ^= u32::from(*b);
                for _ in 0..8 {
                    c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
            let body: Vec<u8> = kind.iter().chain(data).copied().collect();
            out.extend((data.len() as u32).to_be_bytes());
            out.extend(&body);
            out.extend(crc(&body).to_be_bytes());
        }
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend(h.to_be_bytes());
        ihdr.extend([8, 2, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &[]);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[test]
    fn huge_pictures_are_not_decoded() {
        // 72 MP: over the cap, though within what the decoder would allocate by default.
        let data = png_header(9_000, 8_000);
        assert_eq!(
            image::ImageReader::with_format(std::io::Cursor::new(&data), image::ImageFormat::Png).into_dimensions().ok(),
            Some((9_000, 8_000))
        );
        assert!(decode(&data, image::ImageFormat::Png).is_none());
        assert!(pack(&data, [0.0; 4], 72.0, 72.0, 96.0, true).is_none());
    }

    /// Compressing a picture whose media is also another picture's artistic-effect original must
    /// keep that media: the effect needs it to be changed or removed.
    #[test]
    fn media_still_used_as_an_artistic_original_is_kept() {
        use deckcraft_model::style::Artistic;
        let mut s = Session::with_new();
        let big = png(1200, 600);
        let processed = png(1100, 600);
        s.edit(|doc, _| {
            let original = doc.add_media("original.png", "image/png", big.clone()).expect("media");
            let shown = doc.add_media("processed.png", "image/png", processed.clone()).expect("media");
            let slide = Arc::make_mut(doc.slides.first_mut().ok_or_else(|| bad("test", "no slide"))?);
            let xfrm = Some(deckcraft_geom::Xfrm::new(0.0, 0.0, 72.0, 36.0));
            let with_effect =
                PictureFill { media: shown, artistic: Some(Artistic { effect: "artisticBlur".into(), original }), ..Default::default() };
            slide.shapes.push(Shape { id: ShapeId(70_001), xfrm, kind: ShapeKind::Picture { fill: with_effect }, ..Default::default() });
            slide.shapes.push(Shape {
                id: ShapeId(70_002),
                xfrm,
                kind: ShapeKind::Picture { fill: PictureFill { media: original, ..Default::default() } },
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
        let r = s.execute("picture.compress", &json!({"all": true, "ppi": 96})).unwrap();
        assert_eq!(r["pictures"], 2, "{r}");
        let doc = &s.doc().unwrap().doc;
        // Every reference left in the deck resolves to media that is still there.
        for id in crate::media_refs::presentation_media(doc) {
            assert!(doc.media(id).is_some(), "{id:?} was deleted while still referenced");
        }
        assert!(doc.media.iter().any(|m| m.data.as_slice() == big.as_slice()), "the artistic original was deleted");
        assert!(!doc.media.iter().any(|m| m.data.as_slice() == processed.as_slice()), "the replaced processed picture is dropped");
    }
}
