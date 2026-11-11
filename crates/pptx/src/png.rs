//! A minimal PNG encoder for the placeholder image written when a picture's media is missing.

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c ^= *b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for x in data {
        a = (a + *x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut c = Vec::with_capacity(data.len() + 4);
    c.extend_from_slice(kind);
    c.extend_from_slice(data);
    out.extend_from_slice(&c);
    out.extend_from_slice(&crc32(&c).to_be_bytes());
}

/// A 1×1 fully transparent RGBA PNG.
pub fn transparent_1x1() -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&1u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    // One scanline: filter byte + RGBA, stored (uncompressed) deflate block.
    let raw = [0u8, 0, 0, 0, 0];
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    z.extend_from_slice(&raw);
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A zlib stream holding `raw` as stored (uncompressed) deflate blocks.
fn store_zlib(raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    let mut rest = raw;
    loop {
        let n = rest.len().min(65_535);
        let last = n == rest.len();
        z.push(u8::from(last));
        z.extend_from_slice(&(n as u16).to_le_bytes());
        z.extend_from_slice(&(!(n as u16)).to_le_bytes());
        z.extend_from_slice(&rest[..n]);
        rest = &rest[n..];
        if last {
            break;
        }
    }
    z.extend_from_slice(&adler32(raw).to_be_bytes());
    z
}

/// An RGBA PNG (`w × h × 4` bytes, row-major), uncompressed.
fn rgba_png(w: u32, h: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for row in pixels.chunks(w as usize * 4) {
        raw.push(0); // filter: none
        raw.extend_from_slice(row);
    }
    let mut ihdr = vec![];
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &store_zlib(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// A 64×64 camera placeholder for a recorded Cameo whose poster part cannot be read: a dark
/// rounded square with a light lens. Opaque PNG, so every renderer draws it (SVG does not).
pub fn cameo_poster() -> Vec<u8> {
    const SIDE: i32 = 64;
    let c = SIDE / 2;
    let mut px = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let d2 = (x - c).pow(2) + (y - c).pow(2);
            let colour = if (12..=22).contains(&d2) {
                [0xE8, 0xEE, 0xF7, 0xFF]
            } else if d2 < 12 {
                [0x8F, 0xD3, 0xFF, 0xFF]
            } else {
                [0x33, 0x40, 0x4F, 0xFF]
            };
            let i = ((y * SIDE + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&colour);
        }
    }
    rgba_png(SIDE as u32, SIDE as u32, &px)
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc_known() {
        assert_eq!(super::crc32(b"IEND"), 0xAE42_6082);
        assert!(super::transparent_1x1().starts_with(b"\x89PNG"));
    }

    #[test]
    fn the_cameo_poster_is_an_opaque_png() {
        let png = super::cameo_poster();
        assert!(png.starts_with(b"\x89PNG"));
        let img = image::load_from_memory(&png).expect("decodes").to_rgba8();
        assert_eq!(img.dimensions(), (64, 64));
        assert!(img.pixels().any(|p| p.0[3] == 255), "it is opaque");
        assert!(img.pixels().any(|p| p.0[0] != img.get_pixel(0, 0).0[0]), "it is not one flat colour");
    }
}
