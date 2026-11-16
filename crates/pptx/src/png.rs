//! The placeholder image written when a picture's media is missing: a 1×1 fully transparent RGBA
//! PNG, kept as its fixed bytes (signature, IHDR, one stored-deflate IDAT scanline, IEND) so the
//! app carries no PNG encoder of its own.

/// The PNG file, byte for byte what the former local encoder produced.
const TRANSPARENT_1X1: [u8; 73] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x10, 0x49, 0x44, 0x41, 0x54, 0x78, 0x01, 0x01, 0x05, 0x00, 0xFA, 0xFF,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x01, 0x64, 0x78, 0x95, 0x38, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
    0x82,
];

/// A 1×1 fully transparent RGBA PNG.
pub fn transparent_1x1() -> Vec<u8> {
    TRANSPARENT_1X1.to_vec()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    #[test]
    fn placeholder_is_a_transparent_1x1_png() {
        let png = super::transparent_1x1();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        // IHDR: 1×1, 8-bit RGBA, no interlace.
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..29], &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        assert!(png.ends_with(b"IEND\xAE\x42\x60\x82"));
        // The stored scanline: filter byte 0, then one pixel with zero alpha.
        assert_eq!(&png[49..54], &[0, 0, 0, 0, 0]);
    }
}
