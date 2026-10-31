//! System pasteboard: HTML rich text, and a picture (TIFF on macOS, via arboard).
//!
//! Pages keeps bold, italic and underline when the pasteboard carries HTML plus the same plain
//! words. A shape with no text is copied as an image by the UI; this module only reads a picture
//! back (Keynote's TIFF) as PNG bytes.

/// Keep the selection owner alive between operations on Linux. Opening the clipboard is lazy,
/// so starting the app without clipboard access is harmless and failed opens can be retried.
#[derive(Default)]
pub struct Pasteboard {
    clipboard: Option<arboard::Clipboard>,
}

fn retained<T>(slot: &mut Option<T>, create: impl FnOnce() -> Option<T>) -> Option<&mut T> {
    if slot.is_none() {
        *slot = create();
    }
    slot.as_mut()
}

impl Pasteboard {
    /// HTML currently on the pasteboard, when there is some.
    pub fn html(&mut self) -> Option<String> {
        let cb = retained(&mut self.clipboard, || arboard::Clipboard::new().ok())?;
        let html = cb.get().html().ok()?;
        let html = html.trim();
        if html.is_empty() { None } else { Some(html.to_string()) }
    }

    /// Write `html` and the plain alternative `text`. `false` when the pasteboard refuses it.
    pub fn set_html(&mut self, html: &str, text: &str) -> bool {
        let Some(cb) = retained(&mut self.clipboard, || arboard::Clipboard::new().ok()) else { return false };
        cb.set_html(html, Some(text)).is_ok()
    }

    /// The pasteboard picture as PNG bytes (TIFF on macOS is decoded by arboard).
    pub fn image_png(&mut self) -> Option<Vec<u8>> {
        let cb = retained(&mut self.clipboard, || arboard::Clipboard::new().ok())?;
        let img = cb.get_image().ok()?;
        let width = u32::try_from(img.width).ok()?;
        let height = u32::try_from(img.height).ok()?;
        let rgba = image::RgbaImage::from_raw(width, height, img.bytes.into_owned())?;
        let mut out = Vec::new();
        rgba.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    #[test]
    fn desktop_clipboard_retains_owner_and_retries_failed_open() {
        let mut owner = None;
        assert!(super::retained::<usize>(&mut owner, || None).is_none());
        assert_eq!(super::retained(&mut owner, || Some(42)), Some(&mut 42));
        // A second operation must reuse the first selection owner, not reopen it.
        assert_eq!(super::retained(&mut owner, || Some(99)), Some(&mut 42));
        assert_eq!(owner, Some(42));
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
        let mut owner = super::Pasteboard::default();
        assert!(owner.set_html("<b>DeckCraft</b>", "DeckCraft"), "set html");
        let html = owner.html().unwrap_or_default();
        assert!(html.contains("<b>") || html.contains("<B>"), "{html}");
        let text = arboard::Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()).unwrap_or_default();
        assert_eq!(text, "DeckCraft");
        let mut cb = arboard::Clipboard::new().expect("pasteboard");
        let img = arboard::ImageData { width: 1, height: 1, bytes: Cow::Owned(vec![255, 0, 0, 255]) };
        cb.set_image(img).expect("set image");
        drop(cb);
        let png = owner.image_png().expect("png");
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
