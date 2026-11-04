//! System pasteboard: HTML rich text, and a picture (TIFF on macOS, via arboard).
//!
//! Pages keeps bold, italic and underline when the pasteboard carries HTML plus the same plain
//! words. A shape with no text is copied as an image by the UI; this module only reads a picture
//! back (Keynote's TIFF) as PNG bytes.

/// Plain text currently on the pasteboard, when there is some.
pub fn text() -> Option<String> {
    arboard_text().or_else(platform_text)
}

fn arboard_text() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let text = cb.get_text().ok()?.replace("\r\n", "\n");
    if text.is_empty() { None } else { Some(text) }
}

fn platform_text() -> Option<String> {
    #[cfg(windows)]
    {
        windows_text()
    }
    #[cfg(not(windows))]
    {
        None
    }
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
///
/// On Windows a screenshot is often `CF_DIB` or `CF_BITMAP`, which arboard does not read.
pub fn image_png() -> Option<Vec<u8>> {
    arboard_image_png().or_else(platform_image_png)
}

fn arboard_image_png() -> Option<Vec<u8>> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let img = cb.get_image().ok()?;
    let width = u32::try_from(img.width).ok()?;
    let height = u32::try_from(img.height).ok()?;
    let rgba = image::RgbaImage::from_raw(width, height, img.bytes.into_owned())?;
    encode_rgba(&rgba)
}

fn platform_image_png() -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        windows_image_png()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn encode_rgba(rgba: &image::RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    rgba.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

/// Same bound as a paste into the document: 16 megapixels.
#[cfg(any(windows, test))]
const MAX_PASTE_PIXELS: usize = 16_000_000;
#[cfg(any(windows, test))]
const MAX_DIB_BYTES: usize = 70 * 1024 * 1024;

/// `CF_DIB` is a `BITMAPINFO` plus pixels, without the 14-byte file header a `.bmp` has.
#[cfg(any(windows, test))]
fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 || dib.len() > MAX_DIB_BYTES {
        return None;
    }
    let header = u32::from_le_bytes(dib.get(..4)?.try_into().ok()?) as usize;
    if header < 40 || header > dib.len() {
        return None;
    }
    let width = i32::from_le_bytes(dib.get(4..8)?.try_into().ok()?);
    let height = i32::from_le_bytes(dib.get(8..12)?.try_into().ok()?);
    let planes = u16::from_le_bytes(dib.get(12..14)?.try_into().ok()?);
    let bits = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
    let compression = u32::from_le_bytes(dib.get(16..20)?.try_into().ok()?);
    // BI_RGB = 0, BI_BITFIELDS = 3. RLE and embedded JPEG stay unread.
    if planes != 1 || width <= 0 || height == 0 || (compression != 0 && compression != 3) {
        return None;
    }
    let pixels = usize::try_from(width).ok()?.checked_mul(usize::try_from(height.unsigned_abs()).ok()?)?;
    if pixels == 0 || pixels > MAX_PASTE_PIXELS {
        return None;
    }
    let clr_used = u32::from_le_bytes(dib.get(32..36)?.try_into().ok()?) as usize;
    let table = dib_color_table(header, bits, compression, clr_used)?;
    let pixel_at = header.checked_add(table)?;
    if dib.len() < pixel_at {
        return None;
    }
    let file_size = 14usize.checked_add(dib.len())?;
    if file_size > MAX_DIB_BYTES {
        return None;
    }
    let mut bmp = Vec::with_capacity(file_size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&u32::try_from(file_size).ok()?.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    let off = 14usize.checked_add(pixel_at)?;
    bmp.extend_from_slice(&u32::try_from(off).ok()?.to_le_bytes());
    bmp.extend_from_slice(dib);
    let rgba = image::load_from_memory(&bmp).ok()?.into_rgba8();
    let decoded = (rgba.width() as usize).checked_mul(rgba.height() as usize)?;
    if decoded == 0 || decoded > MAX_PASTE_PIXELS {
        return None;
    }
    encode_rgba(&rgba)
}

#[cfg(any(windows, test))]
fn dib_color_table(header: usize, bits: u16, compression: u32, clr_used: usize) -> Option<usize> {
    if bits <= 8 {
        let n = if clr_used == 0 { 1usize.checked_shl(u32::from(bits))? } else { clr_used };
        return n.checked_mul(4);
    }
    if compression == 3 && header < 108 {
        return if clr_used == 0 { Some(12) } else { clr_used.checked_mul(4) };
    }
    Some(0)
}

#[cfg(windows)]
fn bmp_file_to_png(bmp: &[u8]) -> Option<Vec<u8>> {
    if bmp.len() < 54 || bmp.get(..2) != Some(b"BM".as_slice()) {
        return None;
    }
    dib_to_png(bmp.get(14..)?)
}

#[cfg(windows)]
fn windows_text() -> Option<String> {
    if let Ok(text) = clipboard_win::get_clipboard_string() {
        let text = text.trim_end_matches('\0').replace("\r\n", "\n");
        if !text.is_empty() {
            return Some(text);
        }
    }
    let bytes: Vec<u8> = clipboard_win::get_clipboard(clipboard_win::formats::RawData(clipboard_win::formats::CF_TEXT)).ok()?;
    decode_clipboard_bytes(&bytes)
}

#[cfg(windows)]
fn windows_image_png() -> Option<Vec<u8>> {
    if let Some(dib) = clipboard_dib()
        && let Some(png) = dib_to_png(&dib)
    {
        return Some(png);
    }
    let bmp: Vec<u8> = clipboard_win::get_clipboard(clipboard_win::formats::Bitmap).ok()?;
    bmp_file_to_png(&bmp)
}

#[cfg(windows)]
fn clipboard_dib() -> Option<Vec<u8>> {
    clipboard_win::get_clipboard(clipboard_win::formats::RawData(clipboard_win::formats::CF_DIB)).ok()
}

#[cfg(any(windows, test))]
fn decode_clipboard_bytes(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let bytes = bytes.get(..end)?;
    if bytes.is_empty() {
        return None;
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|b| char::from(*b)).collect(),
    };
    let text = text.replace("\r\n", "\n");
    if text.is_empty() { None } else { Some(text) }
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

    /// Windows screenshots are a `CF_DIB`: a bitmap header and pixels, with no file header.
    #[test]
    fn a_device_independent_bitmap_becomes_png() {
        let mut dib = vec![0u8; 44];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&24u16.to_le_bytes());
        dib[20..24].copy_from_slice(&4u32.to_le_bytes());
        dib[40] = 0;
        dib[41] = 0;
        dib[42] = 255;
        let png = super::dib_to_png(&dib).expect("png");
        let decoded = image::load_from_memory(&png).expect("decode").to_rgba8();
        let p = decoded.get_pixel(0, 0);
        assert!(p[0] > 200 && p[1] < 40 && p[2] < 40 && p[3] > 200, "{p:?}");
        assert!(super::dib_to_png(&[0, 1, 2]).is_none());
        let mut huge = vec![0u8; 40];
        huge[0..4].copy_from_slice(&40u32.to_le_bytes());
        huge[4..8].copy_from_slice(&20_000i32.to_le_bytes());
        huge[8..12].copy_from_slice(&20_000i32.to_le_bytes());
        huge[12..14].copy_from_slice(&1u16.to_le_bytes());
        huge[14..16].copy_from_slice(&24u16.to_le_bytes());
        assert!(super::dib_to_png(&huge).is_none());
    }

    #[test]
    fn ansi_clipboard_bytes_keep_every_byte() {
        assert_eq!(super::decode_clipboard_bytes(b"Caf\xE9\0"), Some("Café".into()));
        assert_eq!(super::decode_clipboard_bytes(b"a\r\nb"), Some("a\nb".into()));
        assert_eq!(super::decode_clipboard_bytes(&[0]), None);
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
