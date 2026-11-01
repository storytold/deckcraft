//! Embedded SVG rendering. No file/network image resolver or system-font scan is enabled.
use resvg::{tiny_skia, usvg};
use std::sync::Arc;

const MAX_INPUT: usize = 8 << 20;
const MAX_SIDE: u32 = 4096;

pub(crate) fn is_svg(bytes: &[u8]) -> bool {
    let prefix = bytes.get(..bytes.len().min(1024)).unwrap_or(bytes);
    String::from_utf8_lossy(prefix).trim_start_matches('\u{feff}').trim_start().starts_with('<')
}

/// Reject DTDs, deep trees and excessive source nodes before the SVG parser expands references.
fn preflight(bytes: &[u8]) -> Option<()> {
    use quick_xml::events::Event;
    if bytes.len() > MAX_INPUT {
        return None;
    }
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let (mut depth, mut nodes) = (0usize, 0usize);
    loop {
        match reader.read_event().ok()? {
            Event::Start(_) => {
                depth += 1;
                nodes += 1;
            }
            Event::Empty(_) => nodes += 1,
            Event::End(_) => depth = depth.checked_sub(1)?,
            Event::DocType(_) => return None,
            Event::Eof => return (depth == 0).then_some(()),
            _ => {}
        }
        if depth > 64 || nodes > 10000 {
            return None;
        }
    }
}

fn load_face(db: &mut Arc<usvg::fontdb::Database>, face: &deckcraft_fonts::FontFace) -> Option<usvg::fontdb::ID> {
    if let Some(f) = db.faces().find(|f| {
        f.index == face.index()
            && match &f.source {
                usvg::fontdb::Source::Binary(bytes) => bytes.as_ref().as_ref() == face.data(),
            }
    }) {
        return Some(f.id);
    }
    let used = db
        .faces()
        .map(|f| match &f.source {
            usvg::fontdb::Source::Binary(bytes) => bytes.as_ref().as_ref().len(),
        })
        .sum::<usize>();
    if db.len() >= 64 || used.saturating_add(face.data().len()) > 64 << 20 || face.data().len() > 40 << 20 {
        return None;
    }
    let ids = Arc::make_mut(db).load_font_source(usvg::fontdb::Source::Binary(Arc::new(face.data().to_vec())));
    ids.iter().copied().find(|id| db.face(*id).is_some_and(|f| f.index == face.index()))
}

fn document_font(pres: Option<&deckcraft_model::Presentation>, family: &str, style: &str) -> Option<Arc<deckcraft_fonts::FontFace>> {
    let p = pres?;
    let font = p.embedded_fonts.iter().find(|f| f.typeface.eq_ignore_ascii_case(family))?;
    let slot = match style {
        "Bold Italic" => font.bold_italic.or(font.bold).or(font.italic),
        "Bold" => font.bold,
        "Italic" => font.italic,
        _ => font.regular,
    };
    let data = p.media(slot.or(font.regular)?)?.data.clone();
    deckcraft_fonts::embedded_face(data, family, style)
}

fn options(pres: Option<&deckcraft_model::Presentation>) -> usvg::Options<'_> {
    let mut options = usvg::Options { font_family: deckcraft_fonts::DEFAULT_FAMILY.into(), ..Default::default() };
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let data_resolver = usvg::ImageHrefResolver::default_data_resolver();
    options.image_href_resolver.resolve_data = Box::new(move |mime, data, opts| {
        // Nested vector data URLs could recursively evade the outer XML/input budget.
        if !matches!(mime, "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp") || data.len() > MAX_INPUT {
            return None;
        }
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&*data)).with_guessed_format().ok()?.into_dimensions().ok()?;
        if u64::from(w) * u64::from(h) > 16_777_216 {
            return None;
        }
        data_resolver(mime, data, opts)
    });
    options.font_resolver.select_font = Box::new(move |font, db| {
        let source = deckcraft_fonts::FontDb::global();
        let family = font
            .families()
            .iter()
            .find_map(|f| match f {
                usvg::FontFamily::Named(name)
                    if pres.is_some_and(|p| p.embedded_fonts.iter().any(|f| f.typeface.eq_ignore_ascii_case(name)))
                        || source.has_family(name)
                        || !deckcraft_fonts::substitutes(name).is_empty() =>
                {
                    Some(name.as_str())
                }
                usvg::FontFamily::Monospace => Some("Liberation Mono"),
                usvg::FontFamily::Serif => Some("Liberation Serif"),
                _ => None,
            })
            .unwrap_or(deckcraft_fonts::DEFAULT_FAMILY);
        let style = match (font.weight() >= 600, font.style() != usvg::FontStyle::Normal) {
            (true, true) => "Bold Italic",
            (true, false) => "Bold",
            (false, true) => "Italic",
            _ => "Regular",
        };
        let face = document_font(pres, family, style).unwrap_or_else(|| source.document_face(family, style));
        load_face(db, &face)
    });
    let default_fallback = usvg::FontResolver::default_fallback_selector();
    options.font_resolver.select_fallback = Box::new(move |ch, excluded, db| {
        if let Some(id) = default_fallback(ch, excluded, db) {
            return Some(id);
        }
        let face = deckcraft_fonts::FontDb::global().fallback_for(ch, u32::MAX)?;
        load_face(db, &face).filter(|id| !excluded.contains(id))
    });
    options
}

fn tree(bytes: &[u8], pres: Option<&deckcraft_model::Presentation>) -> Option<usvg::Tree> {
    preflight(bytes)?;
    usvg::Tree::from_data(bytes, &options(pres)).ok()
}

pub(crate) fn natural_size(bytes: &[u8]) -> Option<(f64, f64)> {
    let tree = tree(bytes, None)?;
    Some((f64::from(tree.size().width()), f64::from(tree.size().height())))
}

pub(crate) fn rasterize(bytes: &[u8], side: u32) -> Option<image::RgbaImage> {
    rasterize_with_fonts(bytes, side, None)
}

pub(crate) fn rasterize_with_fonts(bytes: &[u8], side: u32, pres: Option<&deckcraft_model::Presentation>) -> Option<image::RgbaImage> {
    let tree = tree(bytes, pres)?;
    let size = tree.size();
    let factor = side.clamp(1, MAX_SIDE) as f32 / size.width().max(size.height());
    let w = (size.width() * factor).round().max(1.0) as u32;
    let h = (size.height() * factor).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(factor, factor), &mut pixmap.as_mut());
    // tiny-skia is premultiplied; the common picture-adjustment path expects straight RGBA.
    let mut pixels = pixmap.take();
    for p in pixels.chunks_exact_mut(4) {
        let a = u16::from(p[3]);
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((u16::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(w, h, pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vectors_scale_and_keep_alpha_and_text() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs><rect width="100" height="100" fill="url(#g)" opacity=".5"/><text x="110" y="60" font-size="35" fill="black">AB</text></svg>"##;
        let img = rasterize(svg, 800).unwrap();
        assert_eq!(img.dimensions(), (800, 400));
        assert_eq!(natural_size(svg), Some((200.0, 100.0)));
        let p = img.get_pixel(10, 10);
        assert!(p[0] > 230 && (120..=135).contains(&p[3]), "{p:?}");
        assert!(img.enumerate_pixels().filter(|(x, _, p)| *x > 400 && p[3] > 200).count() > 1000);
    }
    #[test]
    fn refuses_entities_deep_trees_and_external_images() {
        assert!(rasterize(br#"<!DOCTYPE svg [<!ENTITY x "bad">]><svg xmlns="http://www.w3.org/2000/svg"/>"#, 100).is_none());
        let deep =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{}{}<rect width=\"10\" height=\"10\"/></svg>", "<g>".repeat(65), "</g>".repeat(65));
        assert!(rasterize(deep.as_bytes(), 100).is_none());
        let svg=br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100"><image xlink:href="file:///untrusted-image.png" width="100" height="100"/><image xlink:href="https://example.invalid/image.png" width="100" height="100"/></svg>"#;
        assert!(rasterize(svg, 100).unwrap().pixels().all(|p| p[3] == 0));
    }

    #[test]
    fn existing_local_image_is_not_read_by_svg() {
        let path = std::env::temp_dir().join(format!("deckcraft-svg-no-file-{}.png", std::process::id()));
        image::RgbaImage::from_pixel(10, 10, image::Rgba([255, 0, 0, 255])).save(&path).unwrap();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100"><image xlink:href="{}" width="100" height="100"/></svg>"#,
            path.display()
        );
        let result = rasterize(svg.as_bytes(), 100);
        std::fs::remove_file(&path).unwrap();
        assert!(result.unwrap().pixels().all(|p| p[3] == 0));
    }
}
