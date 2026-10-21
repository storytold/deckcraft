//! Pictures ([MS-ODRAW] §2.2.32–2.2.45): the drawing group's blip store lists every picture
//! (`FBSE`), either holding its blip or pointing into the `Pictures` stream. Shapes name a picture
//! by its 1-based index in the store.

use crate::doc::rt;
use crate::record::Rec;

/// Largest picture inflated from a compressed metafile.
const MAX_INFLATED: usize = 64 << 20;
/// Most bytes all of a file's pictures may decode to together (the native format's media budget).
const MAX_TOTAL: usize = 4 << 30;

/// A decoded picture: file extension, content type and bytes as a file of that type.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub ext: &'static str,
    pub content_type: &'static str,
    pub data: Vec<u8>,
}

/// The blip store: each entry's blip record, decoded only when a shape first asks for it, every
/// entry at most once, and all of them together within [`MAX_TOTAL`] bytes.
pub struct Store<'a> {
    entries: Vec<Option<Rec<'a>>>,
    budget: usize,
}

impl<'a> Store<'a> {
    pub fn new(dgg: Option<Rec<'a>>, pictures_stream: &'a [u8]) -> Self {
        let entries = match dgg.and_then(|d| d.child(rt::BSTORE)) {
            Some(bstore) => bstore.children().map(|r| if r.kind == rt::FBSE { fbse(&r, pictures_stream) } else { None }).collect(),
            None => vec![],
        };
        Store { entries, budget: MAX_TOTAL }
    }

    /// The picture at `index` (0-based; shapes use 1-based ids), taken out of the store. `None`
    /// for an entry that is empty, can't be read, was taken already, or would pass the budget.
    pub fn take(&mut self, index: usize) -> Option<Picture> {
        let blip = self.entries.get_mut(index)?.take()?;
        let pic = decode(&blip, self.budget)?;
        self.budget = self.budget.saturating_sub(pic.data.len());
        Some(pic)
    }
}

fn fbse<'a>(r: &Rec<'a>, pictures: &'a [u8]) -> Option<Rec<'a>> {
    // btWin32, btMacOS, rgbUid[16], tag, size, cRef, foDelay, unused, cbName, unused, unused.
    let size = r.u32(20)? as usize;
    let offset = r.u32(28)? as usize;
    let name_len = usize::from(r.u8(33)?);
    if size == 0 {
        return None;
    }
    let embedded_at = 36usize.checked_add(name_len)?;
    if r.data.len() > embedded_at { Rec::at(r.data, embedded_at) } else { Rec::at(pictures, offset) }
}

/// Decode one blip record into at most `limit` bytes.
pub fn decode(b: &Rec, limit: usize) -> Option<Picture> {
    // The instance says whether a second UID follows the first.
    let (kind, two_uids): (u16, bool) = match b.instance {
        0x3D4 | 0x216 | 0x542 | 0x46A | 0x6E2 | 0x6E0 | 0x7A8 | 0x6E4 => (b.kind, false),
        0x3D5 | 0x217 | 0x543 | 0x46B | 0x6E3 | 0x6E1 | 0x7A9 | 0x6E5 => (b.kind, true),
        _ => (b.kind, false),
    };
    let uids = if two_uids { 32 } else { 16 };
    // The bytes a picture is copied from, refused before copying when they (plus `extra` bytes of
    // header to add) would pass the budget.
    let within = |bytes: &'_ [u8], extra: usize| bytes.len().checked_add(extra).is_some_and(|n| n <= limit);
    let bitmap = || b.data.get(uids + 1..).filter(|d| within(d, 0)).map(<[u8]>::to_vec);
    match kind {
        // EMF, WMF: a metafile header then (usually deflated) data.
        0xF01A | 0xF01B => {
            let h = b.data.get(uids..uids + 34)?;
            let size = crate::cfb::u32_at(h, 0)? as usize;
            let compressed = *h.get(32)? == 0;
            let body = b.data.get(uids + 34..)?;
            let data = if compressed {
                miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(body, size.min(MAX_INFLATED).min(limit)).ok()?
            } else {
                body.get(..size.min(body.len())).filter(|d| within(d, 0))?.to_vec()
            };
            let (ext, content_type) = if kind == 0xF01A { ("emf", "image/x-emf") } else { ("wmf", "image/x-wmf") };
            Some(Picture { ext, content_type, data })
        }
        // Bitmaps: a one-byte tag then the file.
        0xF01D | 0xF02A => Some(Picture { ext: "jpg", content_type: "image/jpeg", data: bitmap()? }),
        0xF01E => Some(Picture { ext: "png", content_type: "image/png", data: bitmap()? }),
        0xF029 => Some(Picture { ext: "tif", content_type: "image/tiff", data: bitmap()? }),
        0xF01F => Some(Picture { ext: "bmp", content_type: "image/bmp", data: dib_to_bmp(b.data.get(uids + 1..).filter(|d| within(d, 14))?)? }),
        // PICT (Macintosh) has no decoder here.
        _ => None,
    }
    .filter(|p| p.data.len() <= limit)
}

/// A device-independent bitmap with the 14-byte file header a `.bmp` needs in front.
fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    let header = crate::cfb::u32_at(dib, 0)? as usize;
    let (bits, compression, used) = if header == 12 {
        (usize::from(crate::cfb::u16_at(dib, 10)?), 0, 0)
    } else {
        (usize::from(crate::cfb::u16_at(dib, 14)?), crate::cfb::u32_at(dib, 16)?, crate::cfb::u32_at(dib, 32)? as usize)
    };
    let entry = if header == 12 { 3 } else { 4 };
    let colors = if used > 0 {
        used.min(256)
    } else if bits <= 8 {
        1 << bits
    } else {
        0
    };
    // BI_BITFIELDS with the 40-byte header: three masks follow it.
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let offset = 14usize.checked_add(header)?.checked_add(colors * entry)?.checked_add(masks)?;
    let total = u32::try_from(14 + dib.len()).ok()?;
    let mut out = Vec::with_capacity(dib.len() + 14);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
    out.extend_from_slice(dib);
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::record::build::*;

    fn decode_all(b: &Rec) -> Option<Picture> {
        decode(b, usize::MAX)
    }

    /// Many entries pointing at one deflated metafile decode within the shared budget, each
    /// at most once.
    #[test]
    fn the_store_decodes_lazily_within_a_budget() {
        let wmf = vec![7u8; 1000];
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&wmf, 6);
        let mut header = vec![0u8; 34];
        header[..4].copy_from_slice(&(wmf.len() as u32).to_le_bytes());
        let pictures = atom(0xF01B, 0x216, &[&[0u8; 16][..], &header, &z].concat());
        let mut fbse = vec![0u8; 36];
        fbse[20..24].copy_from_slice(&(pictures.len() as u32).to_le_bytes());
        let entries: Vec<Vec<u8>> = (0..5).map(|_| atom(rt::FBSE, 3, &fbse)).collect();
        let dgg = container(rt::DGG_CONTAINER, 0, &[container(rt::BSTORE, 5, &entries)]);
        let mut store = Store::new(Rec::at(&dgg, 0), &pictures);
        store.budget = 2500;
        assert_eq!(store.take(0).map(|p| p.data.len()), Some(1000));
        assert!(store.take(0).is_none(), "taken already");
        assert!(store.take(1).is_some());
        assert!(store.take(2).is_none(), "past the budget");
        assert!(store.take(99).is_none());
    }

    #[test]
    fn decodes_png_and_deflated_metafiles() {
        let png = [&[0u8; 17][..], b"\x89PNG..."].concat();
        let p = decode_all(&Rec::at(&atom(0xF01E, 0x6E0, &png), 0).unwrap()).unwrap();
        assert_eq!((p.ext, &p.data[..4]), ("png", &b"\x89PNG"[..]));
        // Over the remaining budget: refused before it is copied.
        assert!(decode(&Rec::at(&atom(0xF01E, 0x6E0, &png), 0).unwrap(), 6).is_none());

        let wmf = b"\x01\x00\x09\x00\x00\x03 a tiny metafile".to_vec();
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&wmf, 6);
        let mut header = vec![0u8; 34];
        header[..4].copy_from_slice(&(wmf.len() as u32).to_le_bytes());
        header[28..32].copy_from_slice(&(z.len() as u32).to_le_bytes());
        let body = [&[0u8; 16][..], &header, &z].concat();
        let p = decode_all(&Rec::at(&atom(0xF01B, 0x216, &body), 0).unwrap()).unwrap();
        assert_eq!((p.ext, p.data), ("wmf", wmf));
        // Damaged deflate data is no picture rather than a panic.
        let bad = [&[0u8; 16][..], &header, &[1, 2, 3]].concat();
        assert!(decode_all(&Rec::at(&atom(0xF01A, 0x3D4, &bad), 0).unwrap()).is_none());
    }

    #[test]
    fn a_dib_becomes_a_bmp_the_image_crate_reads() {
        // 1×1, 24-bit.
        let mut dib = vec![0u8; 40];
        dib[0] = 40;
        dib[4] = 1;
        dib[8] = 1;
        dib[12] = 1;
        dib[14] = 24;
        dib.extend_from_slice(&[0, 0, 255, 0]);
        let body = [&[0u8; 17][..], &dib].concat();
        let p = decode_all(&Rec::at(&atom(0xF01F, 0x7A8, &body), 0).unwrap()).unwrap();
        assert_eq!(&p.data[..2], b"BM");
        assert_eq!(u32::from_le_bytes(p.data[10..14].try_into().unwrap()), 54);
        let img = image::load_from_memory_with_format(&p.data, image::ImageFormat::Bmp).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (1, 1));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }
}
