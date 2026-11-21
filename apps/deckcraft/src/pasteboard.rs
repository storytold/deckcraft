//! System pasteboard: HTML rich text, and a picture (TIFF on macOS, via arboard).
//!
//! Pages keeps bold, italic and underline when the pasteboard carries HTML plus the same plain
//! words. A shape with no text is copied as an image by the UI; this module only reads a picture
//! back (Keynote's TIFF) as PNG bytes.

/// Run `f` with a pasteboard handle.
///
/// On X11 and Wayland the copied data is served by the handle that set it: dropping the last
/// handle hands the data to a clipboard manager, or loses it when there is none (the usual case
/// on a bare X11 session), so another DeckCraft window would find an empty clipboard. One handle
/// for the life of the process keeps a copy on offer until another app copies. macOS and Windows
/// keep the data themselves, so a handle per call is enough there.
#[cfg(target_os = "linux")]
fn with<T>(f: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
    use std::sync::{Mutex, OnceLock};
    static HANDLE: OnceLock<Mutex<arboard::Clipboard>> = OnceLock::new();
    if HANDLE.get().is_none() {
        let cb = arboard::Clipboard::new().map_err(|e| log::warn!("no system clipboard: {e}")).ok()?;
        let _ = HANDLE.set(Mutex::new(cb));
    }
    let mut cb = HANDLE.get()?.lock().map_err(|_| log::warn!("the system clipboard handle is poisoned")).ok()?;
    Some(f(&mut cb))
}

#[cfg(not(target_os = "linux"))]
fn with<T>(f: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
    let mut cb = arboard::Clipboard::new().ok()?;
    Some(f(&mut cb))
}

/// HTML currently on the pasteboard, when there is some.
pub fn html() -> Option<String> {
    let html = with(|cb| cb.get().html().ok())??;
    let html = html.trim();
    if html.is_empty() { None } else { Some(html.to_string()) }
}

/// Write `html` and the plain alternative `text`. `false` when the pasteboard refuses it.
pub fn set_html(html: &str, text: &str) -> bool {
    with(|cb| cb.set_html(html, Some(text)).is_ok()).unwrap_or(false)
}

/// The pasteboard picture as PNG bytes (TIFF on macOS is decoded by arboard).
pub fn image_png() -> Option<Vec<u8>> {
    let img = with(|cb| cb.get_image().ok())??;
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
