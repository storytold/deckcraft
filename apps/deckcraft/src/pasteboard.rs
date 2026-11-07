//! System pasteboard: HTML rich text, and a picture (TIFF on macOS, via arboard).
//!
//! Pages keeps bold, italic and underline when the pasteboard carries HTML plus the same plain
//! words. A shape with no text is copied as an image by the UI; this module only reads a picture
//! back (Keynote's TIFF) as PNG bytes.

#[cfg(target_os = "windows")]
const NATIVE_FORMAT: &str = "DeckCraft.Clipboard.v1";

/// Read under one clipboard lock, with a length prefix because Windows can pad allocations.
#[cfg(target_os = "windows")]
pub fn native() -> Option<Vec<u8>> {
    let format = clipboard_win::register_format(NATIVE_FORMAT)?.get();
    let _lock = clipboard_win::Clipboard::new_attempts(10).ok()?;
    let size = clipboard_win::size(format)?.get();
    if size > deckcraft_engine::clipboard::MAX_BYTES + 4 {
        return None;
    }
    let bytes: Vec<u8> = clipboard_win::get(clipboard_win::formats::RawData(format)).ok()?;
    let len = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    bytes.get(4..4usize.checked_add(len)?).map(<[u8]>::to_vec)
}

/// All formats are offered atomically; do not let egui's later CopyText clear native objects.
#[cfg(target_os = "windows")]
pub fn set_native(bytes: &[u8], html: &str, text: &str) -> bool {
    fn write(bytes: &[u8], html: &str, text: &str) -> Option<()> {
        if bytes.len() > deckcraft_engine::clipboard::MAX_BYTES {
            return None;
        }
        let format = clipboard_win::register_format(NATIVE_FORMAT)?.get();
        let mut framed = (u32::try_from(bytes.len()).ok()?).to_le_bytes().to_vec();
        framed.extend_from_slice(bytes);
        let _lock = clipboard_win::Clipboard::new_attempts(10).ok()?;
        clipboard_win::set(clipboard_win::formats::Unicode, text).ok()?;
        if !html.is_empty() {
            clipboard_win::set(clipboard_win::formats::Html::new()?, html).ok()?;
        }
        clipboard_win::raw::set_without_clear(format, &framed).ok()?;
        Some(())
    }
    write(bytes, html, text).is_some()
}

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

    /// This touches the real OS clipboard: run explicitly, serially, on a desktop session.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "writes the real Windows clipboard"]
    #[allow(clippy::unwrap_used)]
    fn native_clipboard_retains_html_and_plain_text_and_clears_on_external_copy() {
        let _restore = Restore(arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()));
        let bytes = b"native objects and media\0with binary bytes";
        assert!(super::set_native(bytes, "<b>DeckCraft</b>", "DeckCraft"));
        assert_eq!(super::native().as_deref(), Some(bytes.as_slice()));
        assert!(super::html().unwrap().contains("<b>DeckCraft</b>"));
        let mut cb = arboard::Clipboard::new().unwrap();
        assert_eq!(cb.get_text().unwrap(), "DeckCraft");
        cb.set_text("another app").unwrap();
        drop(cb);
        assert!(super::native().is_none(), "an external copy must retire our objects");
        assert!(super::set_native(bytes, "", "\u{2060}"));
        assert_eq!(super::native().as_deref(), Some(bytes.as_slice()));
        assert_eq!(arboard::Clipboard::new().unwrap().get_text().unwrap(), "\u{2060}");
    }

    /// Puts HTML and a 1×1 picture on the macOS pasteboard and reads them back.
    /// Restores whatever plain text was there. Other platforms return immediately.
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
