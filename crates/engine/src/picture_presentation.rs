//! PowerPoint Picture Presentation: a `.pptx` where every slide is one picture.
//!
//! `file.export` with format `picture-presentation`, `save_bytes` and the Save As / Change File
//! Type lists write this. Each slide is rendered once and embedded as a full-slide picture, so
//! fonts, effects and layout can't shift in another app — the cost of that guarantee is that the
//! copy is static: no text, transitions or animations survive.
//!
//! The pictures come from the slide renderer at `scale` pixels per point (144 dpi at the default
//! of 2), bounded the same way `file.export`'s PNG export is.

use std::sync::Arc;

use deckcraft_model::{LayoutType, Presentation, Shape, ShapeId, ShapeKind, defaults, style::PictureFill};

use crate::{EngineError, Result};

/// Render scale in pixels per point when none is given (144 dpi for a 720 pt wide slide).
pub const DEFAULT_SCALE: f64 = 2.0;

/// The `.pptx` bytes of `doc` as one picture per slide. `scale` is px per pt; non-finite or
/// out-of-range values fall back to [`DEFAULT_SCALE`], never panic.
pub fn encode(doc: &Presentation, scale: f64) -> Result<Vec<u8>> {
    deckcraft_pptx::export(&build(doc, scale)?).map_err(|e| EngineError::Other(e.to_string()))
}

/// A copy of `doc` whose slides hold one full-slide picture each. Hidden slides stay hidden.
fn build(doc: &Presentation, scale: f64) -> Result<Presentation> {
    let scale = if scale.is_finite() { scale.clamp(0.01, 16.0) } else { DEFAULT_SCALE };
    let size = doc.slide_size;
    let mut out = defaults::blank_presentation(size, Default::default(), false);
    out.props = doc.props.clone();
    out.first_slide_number = doc.first_slide_number;
    let layout = defaults::layout_of_kind(&out, LayoutType::Blank)
        .or_else(|| out.masters.first().and_then(|m| m.layouts.first()).map(|l| l.id))
        .ok_or_else(|| EngineError::Other("the picture presentation has no layout to put slides on".into()))?;
    for (i, slide) in doc.slides.iter().enumerate() {
        let mut s = defaults::new_slide(&mut out, layout).ok_or_else(|| EngineError::Other(crate::cmd::NO_ID_SPACE.into()))?;
        s.name = slide.name.clone();
        s.hidden = slide.hidden;
        let (png, _, _) = crate::cmd::file::render_png_with(doc, i, scale, false, false);
        let media =
            out.add_media(&format!("slide{}.png", i + 1), "image/png", png).ok_or_else(|| EngineError::Other(crate::cmd::NO_ID_SPACE.into()))?;
        let id = ShapeId(out.alloc_id().ok_or_else(|| EngineError::Other(crate::cmd::NO_ID_SPACE.into()))?);
        let title = slide.title();
        let name = if title.is_empty() { format!("Slide {}", i + 1) } else { title };
        s.shapes.push(Shape {
            id,
            name: name.clone(),
            descr: name,
            xfrm: Some(deckcraft_geom::Xfrm::new(0.0, 0.0, size.width, size.height)),
            kind: ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
            ..Default::default()
        });
        out.slides.push(Arc::new(s));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn source_deck() -> Presentation {
        let mut p = defaults::new_presentation(None);
        let blank = defaults::layout_of_kind(&p, LayoutType::Blank).expect("blank layout");
        let second = defaults::new_slide(&mut p, blank).expect("slide");
        p.slides.push(Arc::new(second));
        Arc::make_mut(&mut p.slides[1]).hidden = true;
        p.props.title = "Deck".into();
        p
    }

    #[test]
    fn each_slide_becomes_one_full_slide_picture() {
        let src = source_deck();
        let bytes = encode(&src, 1.0).expect("encode");
        assert!(deckcraft_pptx::sniff(&bytes), "the copy is a .pptx");
        let back = deckcraft_pptx::import(&bytes).expect("reopen");
        assert_eq!(back.slides.len(), src.slides.len());
        assert_eq!(back.slide_size, src.slide_size);
        for slide in &back.slides {
            assert_eq!(slide.shapes.len(), 1, "one picture per slide");
            let shape = &slide.shapes[0];
            let ShapeKind::Picture { fill } = &shape.kind else { panic!("a slide's only shape is its picture") };
            let x = shape.xfrm.expect("positioned");
            assert_eq!((x.x, x.y, x.w, x.h), (0.0, 0.0, src.slide_size.width, src.slide_size.height));
            let media = back.media(fill.media).expect("media item");
            assert_eq!(media.content_type, "image/png");
            assert!(media.data.starts_with(&[0x89, b'P', b'N', b'G']), "the embedded picture is the rendered slide");
        }
    }

    #[test]
    fn hidden_slides_and_properties_are_kept() {
        let bytes = encode(&source_deck(), 1.0).expect("encode");
        let back = deckcraft_pptx::import(&bytes).expect("reopen");
        assert!(!back.slides[0].hidden);
        assert!(back.slides[1].hidden);
        assert_eq!(back.props.title, "Deck");
    }

    #[test]
    fn save_bytes_and_export_know_the_format() {
        let src = source_deck();
        let bytes = crate::cmd::file::save_bytes(&src, "picture-presentation").expect("save_bytes");
        assert!(deckcraft_pptx::sniff(&bytes));
        assert!(deckcraft_pptx::sniff(&encode(&src, f64::NAN).expect("a bad scale is not fatal")));
        assert!(deckcraft_pptx::sniff(&encode(&src, f64::INFINITY).expect("an infinite scale is not fatal")));
    }
}
