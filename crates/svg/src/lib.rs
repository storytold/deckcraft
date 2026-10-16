//! SVG pictures, rasterized for the slide and for the PNG fallback stored next to an icon.
//!
//! An SVG is not allowed to read a file or a URL. Embedded data-URL images are drawn when they
//! fit the size cap. Text is not drawn: no fonts are loaded.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::io::{Cursor, Read};
use std::sync::Arc;

/// Longest side when the caller does not ask for one.
pub const DEFAULT_SIDE: u32 = 1024;

/// Refuse an SVG larger than this, compressed or inflated. A picture does not need a multi-megabyte document.
const MAX_BYTES: usize = 16 << 20;
const MAX_SIDE: u32 = 2048;
/// An embedded raster larger than this on either side is not decoded.
const MAX_EMBED_SIDE: u32 = 4096;
/// How much of the file to scan for the `<svg` root after the prolog.
const PROLOG_SCAN: usize = 64 << 10;
const MAX_SVG_NEST: u32 = 8;

std::thread_local! {
    static SVG_NEST: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The bytes look like an SVG document: an optional BOM, then XML declarations, comments and a
/// doctype, then `<svg`.
pub fn is_svg(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let head = &bytes[..bytes.len().min(PROLOG_SCAN)];
    let mut i = 0;
    loop {
        i = skip_ws(head, i);
        if i >= head.len() {
            return false;
        }
        if head[i..].starts_with(b"<?") {
            let Some(end) = find_sub(head, i + 2, b"?>") else { return false };
            i = end + 2;
            continue;
        }
        if head[i..].starts_with(b"<!--") {
            let Some(end) = find_sub(head, i + 4, b"-->") else { return false };
            i = end + 3;
            continue;
        }
        if starts_with_ignore_ascii(head, i, b"<!DOCTYPE") {
            let Some(end) = skip_doctype(head, i) else { return false };
            i = end;
            continue;
        }
        break;
    }
    starts_with_ignore_ascii(head, i, b"<svg")
}

/// Gzip-compressed SVG (`.svgz`).
pub fn is_svgz(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1F, 0x8B])
}

/// SVG markup. A `.svgz` is inflated first, and refused when the inflated document exceeds the cap.
pub fn svg_xml(bytes: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return None;
    }
    if is_svg(bytes) {
        return Some(std::borrow::Cow::Borrowed(bytes));
    }
    if is_svgz(bytes) {
        let plain = inflate_limited(bytes)?;
        if is_svg(&plain) {
            return Some(std::borrow::Cow::Owned(plain));
        }
    }
    None
}

/// Width and height in pixels (96 dpi), from the document size. A document larger than
/// [`MAX_SIDE`] on its long side is scaled down uniformly, so the aspect stays.
pub fn pixel_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let size = parse(bytes)?.size();
    let (w, h) = (size.width(), size.height());
    if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 {
        return None;
    }
    let longest = w.max(h);
    let scale = if longest > MAX_SIDE as f32 { MAX_SIDE as f32 / longest } else { 1.0 };
    let dim = |v: f32| (v * scale).round().clamp(1.0, MAX_SIDE as f32) as u32;
    Some((dim(w), dim(h)))
}

/// Straight RGBA, with the longer side `side` pixels.
pub fn rasterize(bytes: &[u8], side: u32) -> Option<image::RgbaImage> {
    let tree = parse(bytes)?;
    let size = tree.size();
    let (sw, sh) = (size.width(), size.height());
    if !(sw.is_finite() && sh.is_finite()) || sw < 1.0 || sh < 1.0 {
        return None;
    }
    let side = side.clamp(1, MAX_SIDE);
    let scale = side as f32 / sw.max(sh);
    let w = (sw * scale).ceil().clamp(1.0, MAX_SIDE as f32) as u32;
    let h = (sh * scale).ceil().clamp(1.0, MAX_SIDE as f32) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let src = pixmap.data();
    if src.len() != (w as usize) * (h as usize) * 4 {
        return None;
    }
    let mut img = image::RgbaImage::new(w, h);
    for (dst, px) in img.pixels_mut().zip(src.as_chunks::<4>().0) {
        let (r, g, b, a) = (px[0], px[1], px[2], px[3]);
        let (r, g, b) = if a == 0 {
            (0, 0, 0)
        } else if a == 255 {
            (r, g, b)
        } else {
            let up = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
            (up(r), up(g), up(b))
        };
        *dst = image::Rgba([r, g, b, a]);
    }
    Some(img)
}

/// A PNG of [`rasterize`], for the fallback picture PowerPoint stores beside an SVG icon.
pub fn rasterize_png(bytes: &[u8], side: u32) -> Option<Vec<u8>> {
    let img = rasterize(bytes, side)?;
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn parse(bytes: &[u8]) -> Option<resvg::usvg::Tree> {
    let xml = svg_xml(bytes)?;
    resvg::usvg::Tree::from_data(xml.as_ref(), &options()).ok()
}

fn options() -> resvg::usvg::Options<'static> {
    let mut opt = resvg::usvg::Options::default();
    // The default string resolver opens a path in the href. Pictures must not read the disk.
    opt.image_href_resolver.resolve_string = Box::new(|_, _| None);
    opt.image_href_resolver.resolve_data = Box::new(resolve_embedded);
    opt
}

/// Inflate gzip, stopping once the output would pass [`MAX_BYTES`].
fn inflate_limited(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut dec = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = dec.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        if out.len().saturating_add(n) > MAX_BYTES {
            return None;
        }
        out.extend_from_slice(&buf[..n]);
    }
    Some(out)
}

fn resolve_embedded(mime: &str, data: Arc<Vec<u8>>, opts: &resvg::usvg::Options) -> Option<resvg::usvg::ImageKind> {
    if data.len() > MAX_BYTES {
        return None;
    }
    match mime {
        "image/jpg" | "image/jpeg" | "image/png" | "image/gif" | "image/webp" => raster_kind(mime, data),
        "image/svg+xml" => nested_svg(&data, opts),
        "text/plain" => raster_kind_sniff(data.clone()).or_else(|| nested_svg(&data, opts)),
        _ => None,
    }
}

fn raster_kind(mime: &str, data: Arc<Vec<u8>>) -> Option<resvg::usvg::ImageKind> {
    if !raster_dims_ok(&data) {
        return None;
    }
    match mime {
        "image/jpg" | "image/jpeg" => Some(resvg::usvg::ImageKind::JPEG(data)),
        "image/png" => Some(resvg::usvg::ImageKind::PNG(data)),
        "image/gif" => Some(resvg::usvg::ImageKind::GIF(data)),
        "image/webp" => Some(resvg::usvg::ImageKind::WEBP(data)),
        _ => None,
    }
}

fn raster_kind_sniff(data: Arc<Vec<u8>>) -> Option<resvg::usvg::ImageKind> {
    if !raster_dims_ok(&data) {
        return None;
    }
    let format = image::ImageReader::new(Cursor::new(data.as_slice())).with_guessed_format().ok()?.format()?;
    match format {
        image::ImageFormat::Jpeg => Some(resvg::usvg::ImageKind::JPEG(data)),
        image::ImageFormat::Png => Some(resvg::usvg::ImageKind::PNG(data)),
        image::ImageFormat::Gif => Some(resvg::usvg::ImageKind::GIF(data)),
        image::ImageFormat::WebP => Some(resvg::usvg::ImageKind::WEBP(data)),
        _ => None,
    }
}

fn raster_dims_ok(data: &[u8]) -> bool {
    let Ok(reader) = image::ImageReader::new(Cursor::new(data)).with_guessed_format() else { return false };
    let Ok((w, h)) = reader.into_dimensions() else { return false };
    w > 0 && h > 0 && w <= MAX_EMBED_SIDE && h <= MAX_EMBED_SIDE
}

fn nested_svg(data: &[u8], opts: &resvg::usvg::Options) -> Option<resvg::usvg::ImageKind> {
    let depth = SVG_NEST.with(|n| n.get());
    if depth >= MAX_SVG_NEST {
        return None;
    }
    SVG_NEST.with(|n| n.set(depth + 1));
    let _guard = NestGuard;
    let plain = svg_xml(data)?;
    let tree = resvg::usvg::Tree::from_data_nested(plain.as_ref(), opts).ok()?;
    Some(resvg::usvg::ImageKind::SVG(tree))
}

struct NestGuard;

impl Drop for NestGuard {
    fn drop(&mut self) {
        SVG_NEST.with(|n| n.set(n.get().saturating_sub(1)));
    }
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

fn find_sub(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| from + p)
}

fn starts_with_ignore_ascii(bytes: &[u8], i: usize, prefix: &[u8]) -> bool {
    bytes.get(i..i.saturating_add(prefix.len())).is_some_and(|rest| rest.eq_ignore_ascii_case(prefix))
}

fn skip_doctype(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    let mut brackets = 0i32;
    let mut quote = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' => quote = Some(c),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            b'>' if brackets == 0 => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(fill: &str) -> Vec<u8> {
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="{fill}"/></svg>"#).into_bytes()
    }

    #[test]
    fn a_rectangle_keeps_its_colour_and_size() {
        let img = rasterize(&icon("red"), 20).unwrap();
        assert_eq!(img.dimensions(), (20, 10));
        let p = img.get_pixel(1, 1).0;
        assert!(p[0] > 200 && p[1] < 30 && p[2] < 30 && p[3] == 255, "{p:?}");
        let (w, h) = pixel_size(&icon("#00ff00")).unwrap();
        assert_eq!((w, h), (20, 10));
    }

    #[test]
    fn a_view_box_sets_the_size() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 20"><rect width="40" height="20" fill="blue"/></svg>"#;
        assert!(is_svg(svg));
        let (w, h) = pixel_size(svg).unwrap();
        assert_eq!((w, h), (40, 20));
        let img = rasterize(svg, 40).unwrap();
        let p = img.get_pixel(0, 0).0;
        assert!(p[2] > 200 && p[0] < 30, "{p:?}");
    }

    #[test]
    fn an_image_href_does_not_read_a_file() {
        let dir = std::env::temp_dir().join(format!("deckcraft-svg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("green.png");
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb([0, 255, 0])))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&png, bytes.into_inner()).unwrap();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="red"/><image href="{}" width="4" height="4"/></svg>"#,
            png.display()
        );
        let img = rasterize(svg.as_bytes(), 4).unwrap();
        let p = img.get_pixel(0, 0).0;
        assert!(p[0] > 200 && p[1] < 30, "the linked file was drawn: {p:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_broken_and_oversized_documents_are_refused() {
        assert!(rasterize(b"", 32).is_none());
        assert!(rasterize(b"<svg", 32).is_none());
        assert!(!is_svg(b"\x89PNG\r\n"));
        assert!(rasterize(&vec![b'x'; MAX_BYTES + 1], 32).is_none());
        let png = rasterize_png(&icon("black"), 16).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }

    #[test]
    fn a_doctype_and_a_comment_still_count_as_svg() {
        let svg = br#"<?xml version="1.0" encoding="UTF-8"?>
<?xml-stylesheet href="s.css" type="text/css"?>
<!-- icon -->
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="red"/></svg>"#;
        assert!(is_svg(svg));
        let img = rasterize(svg, 4).unwrap();
        let p = img.get_pixel(0, 0).0;
        assert!(p[0] > 200 && p[1] < 30, "{p:?}");
    }

    #[test]
    fn a_wide_svg_keeps_its_aspect_when_it_is_larger_than_the_cap() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4000" height="1000"><rect width="4000" height="1000" fill="red"/></svg>"#;
        assert_eq!(pixel_size(svg).unwrap(), (2048, 512));
    }

    #[test]
    fn a_small_svgz_that_inflates_past_the_cap_is_refused() {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        let chunk = [0u8; 8192];
        let mut left = MAX_BYTES + 1;
        while left > 0 {
            let n = left.min(chunk.len());
            enc.write_all(&chunk[..n]).unwrap();
            left -= n;
        }
        let gz = enc.finish().unwrap();
        assert!(gz.len() < 64 * 1024, "gzip of zeros should stay small, was {}", gz.len());
        assert!(is_svgz(&gz));
        assert!(svg_xml(&gz).is_none());
        assert!(rasterize(&gz, 32).is_none());
    }

    #[test]
    fn an_svgz_round_trips_to_its_markup() {
        use std::io::Write;
        let svg = icon("red");
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&svg).unwrap();
        let gz = enc.finish().unwrap();
        let xml = svg_xml(&gz).unwrap();
        assert!(xml.starts_with(b"<svg"));
        let img = rasterize(&gz, 20).unwrap();
        assert_eq!(img.dimensions(), (20, 10));
    }

    #[test]
    fn a_data_url_image_larger_than_the_cap_is_not_drawn() {
        let png = png_header(8000, 8000);
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="red"/><image href="data:image/png;base64,{}" width="4" height="4"/></svg>"#,
            b64(&png)
        );
        let img = rasterize(svg.as_bytes(), 4).unwrap();
        let p = img.get_pixel(0, 0).0;
        assert!(p[0] > 200 && p[1] < 30, "the oversized image was drawn: {p:?}");
    }

    fn b64(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        let mut i = 0;
        while i + 2 < data.len() {
            let n = (u32::from(data[i]) << 16) | (u32::from(data[i + 1]) << 8) | u32::from(data[i + 2]);
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            out.push(T[((n >> 6) & 63) as usize] as char);
            out.push(T[(n & 63) as usize] as char);
            i += 3;
        }
        if i < data.len() {
            let b0 = u32::from(data[i]);
            let b1 = data.get(i + 1).copied().map(u32::from).unwrap_or(0);
            let n = (b0 << 16) | (b1 << 8);
            out.push(T[((n >> 18) & 63) as usize] as char);
            out.push(T[((n >> 12) & 63) as usize] as char);
            if i + 1 < data.len() {
                out.push(T[((n >> 6) & 63) as usize] as char);
                out.push('=');
            } else {
                out.push('=');
                out.push('=');
            }
        }
        out
    }

    fn png_header(w: u32, h: u32) -> Vec<u8> {
        fn crc(data: &[u8]) -> u32 {
            let mut c: u32 = 0xFFFF_FFFF;
            for &b in data {
                c ^= u32::from(b);
                for _ in 0..8 {
                    let bit = c & 1;
                    c >>= 1;
                    if bit == 1 {
                        c ^= 0xEDB8_8320;
                    }
                }
            }
            !c
        }
        let mut chunk = Vec::new();
        chunk.extend_from_slice(b"IHDR");
        chunk.extend_from_slice(&w.to_be_bytes());
        chunk.extend_from_slice(&h.to_be_bytes());
        chunk.extend_from_slice(&[8, 2, 0, 0, 0]);
        let sum = crc(&chunk);
        let mut png = vec![137, 80, 78, 71, 13, 10, 26, 10];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(&chunk);
        png.extend_from_slice(&sum.to_be_bytes());
        png
    }
}
