//! System pasteboard: HTML rich text, and a picture (TIFF on macOS, via arboard).
//!
//! Pages keeps bold, italic and underline when the pasteboard carries HTML plus the same plain
//! words. A shape with no text is copied as an image by the UI; this module reads a picture back
//! (Keynote's TIFF) as PNG bytes, and on macOS writes a copied slide's picture beside its text.

/// HTML currently on the pasteboard, when there is some.
pub fn html() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let html = cb.get().html().ok()?;
    let html = html.trim();
    if html.is_empty() { None } else { Some(html.to_string()) }
}

/// Write `html` and the plain alternative `text`. `false` when the pasteboard refuses it.
pub fn set_html(html: &str, text: &str) -> bool {
    let Ok(mut cb) = arboard::Clipboard::new() else { return false };
    cb.set_html(html, Some(text)).is_ok()
}

/// Write a PNG picture and its plain-text alternative in one go (#1309): other apps paste the
/// picture, and DeckCraft pastes its own copy through the text, since egui-winit drops Ctrl+V
/// when the pasteboard holds only a picture (#182). `false` when either is refused. arboard
/// writes one kind at a time, so this goes to `NSPasteboard` directly.
#[cfg(target_os = "macos")]
pub fn set_image(png: &[u8], text: &str) -> bool {
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::{NSData, NSString};
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    // The values of NSPasteboardTypePNG and NSPasteboardTypeString, read as extern statics only
    // through unsafe code.
    let picture = pb.setData_forType(Some(&NSData::with_bytes(png)), &NSString::from_str("public.png"));
    let plain = pb.setString_forType(&NSString::from_str(text), &NSString::from_str("public.utf8-plain-text"));
    picture && plain
}

/// The pasteboard picture as PNG bytes (TIFF on macOS is decoded by arboard).
pub fn image_png() -> Option<Vec<u8>> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let img = cb.get_image().ok()?;
    let width = u32::try_from(img.width).ok()?;
    let height = u32::try_from(img.height).ok()?;
    let rgba = image::RgbaImage::from_raw(width, height, img.bytes.into_owned())?;
    let mut out = Vec::new();
    rgba.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    /// Puts HTML and a 1×1 picture on the macOS pasteboard and reads them back, then a picture
    /// with its text (#1309). Restores whatever plain text was there. Other platforms return
    /// immediately.
    #[test]
    fn html_and_a_picture_round_trip_on_macos() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let previous = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok());
        let _restore = Restore(previous);
        assert!(super::set_html("<b>DeckCraft</b>", "DeckCraft"), "set html");
        let html = super::html().unwrap_or_default();
        assert!(html.contains("<b>") || html.contains("<B>"), "{html}");
        let text = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()).unwrap_or_default();
        assert_eq!(text, "DeckCraft");
        let mut cb = arboard::Clipboard::new().expect("pasteboard");
        let img = arboard::ImageData { width: 1, height: 1, bytes: Cow::Owned(vec![255, 0, 0, 255]) };
        cb.set_image(img).expect("set image");
        drop(cb);
        let png = super::image_png().expect("png");
        let decoded = image::load_from_memory(&png).expect("decode").to_rgba8();
        let p = decoded.get_pixel(0, 0);
        assert!(p[0] > 200 && p[1] < 40 && p[3] > 200, "{p:?}");

        // #1309: a PNG and its plain text land together, so other apps read the picture and
        // DeckCraft still reads its own text. In this test, not its own: tests run in parallel
        // and share the one pasteboard.
        #[cfg(target_os = "macos")]
        {
            let mut png = Vec::new();
            image::RgbaImage::from_pixel(2, 1, image::Rgba([0, 0, 255, 255]))
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .expect("encode");
            assert!(super::set_image(&png, "\u{2060}"), "set image");
            let text = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()).unwrap_or_default();
            assert_eq!(text, "\u{2060}");
            let back = super::image_png().expect("png");
            let decoded = image::load_from_memory(&back).expect("decode").to_rgba8();
            assert_eq!(decoded.dimensions(), (2, 1));
            let p = decoded.get_pixel(1, 0);
            assert!(p[2] > 200 && p[0] < 40 && p[3] > 200, "{p:?}");
        }
    }

    /// Puts the previous plain text back, including when the test fails halfway.
    struct Restore(Option<String>);

    impl Drop for Restore {
        fn drop(&mut self) {
            let Ok(mut cb) = arboard::Clipboard::new() else { return };
            match self.0.take() {
                Some(t) => {
                    let _ = cb.set_text(t);
                }
                None => {
                    let _ = cb.clear();
                }
            }
        }
    }
}
