//! Hand-built WMF and EMF buffers: each test writes the records it needs and checks the parsed model.

use crate::canvas::Canvas;
use crate::dib::Rgba;
use crate::{Error, Item, MAX_BYTES, MAX_ITEMS, Seg, is_metafile, parse};

fn h16(v: i32) -> Vec<u8> {
    (v as i16).to_le_bytes().to_vec()
}

fn h32(v: u32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn e32(v: i32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn cat(parts: Vec<Vec<u8>>) -> Vec<u8> {
    parts.concat()
}

/// One WMF record: size in words, function, parameters (padded to a word boundary).
fn wrec(func: u16, params: &[u8]) -> Vec<u8> {
    let mut p = params.to_vec();
    if !p.len().is_multiple_of(2) {
        p.push(0);
    }
    let words = ((6 + p.len()) / 2) as u32;
    let mut v = words.to_le_bytes().to_vec();
    v.extend(func.to_le_bytes());
    v.extend(p);
    v
}

/// A WMF file: optional placeable header (bbox left, top, right, bottom in logical units, 1440 per inch),
/// the standard header, the records and an end-of-file record.
fn wmf(records: &[Vec<u8>], placeable: Option<[i32; 4]>) -> Vec<u8> {
    let body: Vec<u8> = records.concat();
    let eof = wrec(0, &[]);
    let mut f = Vec::new();
    if let Some(bb) = placeable {
        f.extend(0x9AC6_CDD7u32.to_le_bytes());
        f.extend(0u16.to_le_bytes());
        for v in bb {
            f.extend((v as i16).to_le_bytes());
        }
        f.extend(1440u16.to_le_bytes());
        f.extend(0u32.to_le_bytes());
        f.extend(0u16.to_le_bytes());
    }
    let total = (f.len() + 18 + body.len() + eof.len()) / 2;
    f.extend(1u16.to_le_bytes());
    f.extend(9u16.to_le_bytes());
    f.extend(0x300u16.to_le_bytes());
    f.extend((total as u32).to_le_bytes());
    f.extend(16u16.to_le_bytes());
    f.extend(0u32.to_le_bytes());
    f.extend(0u16.to_le_bytes());
    f.extend(body);
    f.extend(eof);
    f
}

/// One EMF record: type, size in bytes (payload padded to 4), payload.
fn erec(typ: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = payload.to_vec();
    while !p.len().is_multiple_of(4) {
        p.push(0);
    }
    let mut v = typ.to_le_bytes().to_vec();
    v.extend(((8 + p.len()) as u32).to_le_bytes());
    v.extend(p);
    v
}

/// An EMF file with an 88-byte header (bounds and frame in 0.01 mm), the records and an EOF record.
fn emf(records: &[Vec<u8>], bounds: [i32; 4], frame: [i32; 4]) -> Vec<u8> {
    let body: Vec<u8> = records.concat();
    let eof = erec(14, &[0; 12]);
    let total = 88 + body.len() + eof.len();
    let mut h = vec![0u8; 88];
    h[0..4].copy_from_slice(&1u32.to_le_bytes());
    h[4..8].copy_from_slice(&88u32.to_le_bytes());
    h[8..24].copy_from_slice(&bounds.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
    h[24..40].copy_from_slice(&frame.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
    h[40..44].copy_from_slice(&0x464D_4520u32.to_le_bytes());
    h[44..48].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    h[48..52].copy_from_slice(&(total as u32).to_le_bytes());
    h[52..56].copy_from_slice(&((records.len() + 1) as u32).to_le_bytes());
    h[56..58].copy_from_slice(&1u16.to_le_bytes());
    // 1000 x 500 device pixels for 250 x 125 mm: 0.04 device units per hundredth of a millimetre.
    h[72..76].copy_from_slice(&1000i32.to_le_bytes());
    h[76..80].copy_from_slice(&500i32.to_le_bytes());
    h[80..84].copy_from_slice(&250i32.to_le_bytes());
    h[84..88].copy_from_slice(&125i32.to_le_bytes());
    let mut out = h;
    out.extend(body);
    out.extend(eof);
    out
}

/// A BITMAPINFOHEADER (40 bytes).
fn info_header(w: i32, h: i32, bpp: u16, size_image: u32) -> Vec<u8> {
    cat(vec![h32(40), e32(w), e32(h), h16(1), h16(i32::from(bpp)), h32(0), h32(size_image), e32(0), e32(0), h32(0), h32(0)])
}

/// A 2x2 24-bit bottom-up DIB. File rows are stored bottom first: the bottom image row is blue, green;
/// the top row is red, white. Rows are padded to 4 bytes (8 bytes per row).
fn dib24_bottom_up() -> Vec<u8> {
    let mut v = info_header(2, 2, 24, 16);
    v.extend([255, 0, 0, 0, 255, 0, 0, 0]);
    v.extend([0, 0, 255, 255, 255, 255, 0, 0]);
    v
}

/// Top-down RGBA of [`dib24_bottom_up`]: top-left red, top-right white, bottom-left blue, bottom-right green.
const DIB24_RGBA: [u8; 16] = [255, 0, 0, 255, 255, 255, 255, 255, 0, 0, 255, 255, 0, 255, 0, 255];

fn path_segs(item: &Item) -> (&Vec<Seg>, Option<[u8; 4]>, Option<([u8; 4], f32)>, bool) {
    match item {
        Item::Path { segs, fill, stroke, even_odd, .. } => (segs, *fill, *stroke, *even_odd),
        other => panic!("expected a path: {other:?}"),
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn wmf_rectangle_with_solid_brush() {
    let recs = [
        wrec(0x02FC, &cat(vec![h16(0), h32(0x0000_00FF), h16(0)])),
        wrec(0x012D, &h16(0)),
        wrec(0x041B, &cat(vec![h16(70), h16(110), h16(20), h16(10)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    assert_eq!((p.width, p.height), (100.0, 50.0));
    assert_eq!(p.items.len(), 1);
    let (segs, fill, stroke, even_odd) = path_segs(&p.items[0]);
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(100.0, 0.0), Seg::Line(100.0, 50.0), Seg::Line(0.0, 50.0), Seg::Close]);
    assert_eq!(fill, Some([255, 0, 0, 255]));
    assert_eq!(stroke, Some(([0, 0, 0, 255], 0.0)));
    assert!(even_odd);
}

#[test]
fn wmf_polyline_with_pen() {
    let recs = [
        wrec(0x02FA, &cat(vec![h16(0), h16(4), h16(0), h32(0x0000_FF00)])),
        wrec(0x012D, &h16(0)),
        wrec(0x0325, &cat(vec![h16(3), h16(0), h16(0), h16(50), h16(0), h16(50), h16(40)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    assert_eq!((p.width, p.height), (50.0, 40.0));
    let (segs, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(50.0, 0.0), Seg::Line(50.0, 40.0)]);
    assert_eq!(fill, None);
    assert_eq!(stroke, Some(([0, 255, 0, 255], 4.0)));
}

#[test]
fn wmf_polygon_fill_rule_follows_polyfillmode() {
    let square = cat(vec![h16(4), h16(0), h16(0), h16(10), h16(0), h16(10), h16(10), h16(0), h16(10)]);
    let winding = [wrec(0x0106, &h16(2)), wrec(0x0324, &square)];
    let p = parse(&wmf(&winding, None)).unwrap();
    assert!(!path_segs(&p.items[0]).3, "WINDING fills use the nonzero rule");
    let alternate = [wrec(0x0324, &square)];
    let p = parse(&wmf(&alternate, None)).unwrap();
    let (segs, fill, _, even_odd) = path_segs(&p.items[0]);
    assert!(even_odd, "default ALTERNATE is even-odd");
    assert_eq!(fill, Some([255, 255, 255, 255]));
    assert_eq!(segs.last(), Some(&Seg::Close));
}

#[test]
fn wmf_stretchdib_flips_bottom_up_rows() {
    let params = cat(vec![h32(0x00CC_0020), h16(0), h16(2), h16(2), h16(0), h16(0), h16(10), h16(20), h16(0), h16(0), dib24_bottom_up()]);
    let p = parse(&wmf(&[wrec(0x0F43, &params)], None)).unwrap();
    assert_eq!((p.width, p.height), (20.0, 10.0));
    let Item::Bitmap { rect, width, height, rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rect, &[0.0, 0.0, 20.0, 10.0]);
    assert_eq!((*width, *height), (2, 2));
    assert_eq!(rgba.as_slice(), DIB24_RGBA);
}

#[test]
fn wmf_mapping_window_and_viewport() {
    let recs = [
        wrec(0x0103, &h16(8)),
        wrec(0x020B, &cat(vec![h16(100), h16(100)])),
        wrec(0x020C, &cat(vec![h16(50), h16(50)])),
        wrec(0x020E, &cat(vec![h16(100), h16(100)])),
        wrec(0x0214, &cat(vec![h16(100), h16(100)])),
        wrec(0x0213, &cat(vec![h16(150), h16(150)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    assert_eq!((p.width, p.height), (100.0, 100.0));
    let (segs, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(100.0, 100.0)]);
    assert_eq!(fill, None);
    assert_eq!(stroke, Some(([0, 0, 0, 255], 0.0)));
}

#[test]
fn wmf_placeable_header_sets_frame_in_points() {
    let p = parse(&wmf(&[wrec(0x041B, &cat(vec![h16(1440), h16(2880), h16(0), h16(0)]))], Some([0, 0, 2880, 1440]))).unwrap();
    assert_eq!((p.width, p.height), (144.0, 72.0));
}

#[test]
fn emf_stretchdibits_flips_and_fits_frame() {
    let mut payload = vec![0u8; 16];
    payload.extend(cat(vec![
        e32(0),
        e32(0),
        e32(0),
        e32(0),
        e32(2),
        e32(2),
        e32(80),
        e32(40),
        e32(120),
        e32(16),
        e32(0),
        e32(0x00CC_0020),
        e32(100),
        e32(50),
    ]));
    payload.extend(info_header(2, 2, 24, 16));
    payload.extend(dib24_bottom_up().get(40..).unwrap_or_default());
    let rec = erec(81, &payload);
    // A 25 x 12.5 mm frame is 70.87 x 35.43 points; the bitmap fills the whole frame.
    let file = emf(&[rec], [0, 0, 100, 50], [0, 0, 2500, 1250]);
    let p = parse(&file).unwrap();
    let (w, h) = (2500.0 * 72.0 / 2540.0, 1250.0 * 72.0 / 2540.0);
    assert!(close(p.width, w as f32) && close(p.height, h as f32));
    let Item::Bitmap { rect, width, height, rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert!(rect.iter().zip([0.0, 0.0, w, h]).all(|(a, b)| close(*a, b as f32)));
    assert_eq!((*width, *height), (2, 2));
    assert_eq!(rgba.as_slice(), DIB24_RGBA);
}

#[test]
fn emf_world_transform_scales_geometry() {
    let xf = cat([2.0f32, 0.0, 0.0, 2.0, 0.0, 0.0].iter().map(|v| v.to_le_bytes().to_vec()).collect());
    let recs = [erec(35, &xf), erec(27, &cat(vec![e32(0), e32(0)])), erec(54, &cat(vec![e32(10), e32(5)]))];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    assert_eq!((p.width, p.height), (20.0, 10.0));
    let (segs, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(20.0, 10.0)]);
    assert_eq!(fill, None);
    assert_eq!(stroke, Some(([0, 0, 0, 255], 0.0)));
}

#[test]
fn emf_ellipse_is_four_cubics_back_to_start() {
    let rect = cat(vec![e32(0), e32(0), e32(40), e32(20)]);
    let p = parse(&emf(&[erec(42, &rect)], [0; 4], [0; 4])).unwrap();
    let (segs, fill, _, _) = path_segs(&p.items[0]);
    assert_eq!(segs.len(), 6);
    assert_eq!(segs[0], Seg::Move(40.0, 10.0));
    assert!(matches!(segs[4], Seg::Cubic(_, _, _, _, x, y) if close(x, 40.0) && close(y, 10.0)));
    assert_eq!(segs[5], Seg::Close);
    assert_eq!(fill, Some([255, 255, 255, 255]));
}

#[test]
fn emf_path_bracket_strokeandfill_uses_stock_black_brush() {
    let bounds = vec![0u8; 16];
    let recs = [
        erec(59, &[]),
        erec(27, &cat(vec![e32(0), e32(0)])),
        erec(54, &cat(vec![e32(10), e32(0)])),
        erec(54, &cat(vec![e32(10), e32(10)])),
        erec(61, &[]),
        erec(60, &[]),
        erec(37, &h32(0x8000_0004)),
        erec(63, &bounds),
    ];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 1);
    let (segs, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(10.0, 0.0), Seg::Line(10.0, 10.0), Seg::Close]);
    assert_eq!(fill, Some([0, 0, 0, 255]));
    assert_eq!(stroke, Some(([0, 0, 0, 255], 0.0)));
}

#[test]
fn item_cap_stops_the_parse_and_keeps_the_rest() {
    let rect = wrec(0x041B, &cat(vec![h16(10), h16(10), h16(0), h16(0)]));
    let recs: Vec<Vec<u8>> = (0..MAX_ITEMS + 10).map(|_| rect.clone()).collect();
    let p = parse(&wmf(&recs, None)).unwrap();
    assert_eq!(p.items.len(), MAX_ITEMS);
}

#[test]
fn detection_and_garbage() {
    let w = wmf(&[wrec(0x041B, &cat(vec![h16(1), h16(1), h16(0), h16(0)]))], None);
    let e = emf(&[erec(42, &cat(vec![e32(0), e32(0), e32(4), e32(4)]))], [0; 4], [0; 4]);
    assert!(is_metafile(&w));
    assert!(is_metafile(&e));
    assert!(!is_metafile(b"hello world, not a metafile"));
    assert_eq!(parse(b"hello world, not a metafile").unwrap_err(), Error::NotMetafile);
    assert!(parse(&[]).is_err());
}

/// Fixed-seed LCG (Knuth's constants): the fuzz inputs are reproducible.
struct Lcg(u64);

impl Lcg {
    fn rnd(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
}

#[test]
fn truncated_random_and_mutated_inputs_never_panic() {
    let rect = wrec(0x041B, &cat(vec![h16(10), h16(10), h16(0), h16(0)]));
    let wfile = wmf(
        &[
            wrec(0x02FC, &cat(vec![h16(0), h32(0xFF), h16(0)])),
            wrec(0x012D, &h16(0)),
            rect,
            wrec(0x0325, &cat(vec![h16(2), h16(0), h16(0), h16(5), h16(5)])),
        ],
        Some([0, 0, 20, 10]),
    );
    let mut dib_rec = cat(vec![h32(0x00CC_0020), h16(0), h16(2), h16(2), h16(0), h16(0), h16(10), h16(20), h16(0), h16(0)]);
    dib_rec.extend(dib24_bottom_up());
    let mut payload = vec![0u8; 16];
    payload.extend(cat(vec![e32(0), e32(0), e32(0), e32(0), e32(2), e32(2), e32(80), e32(40), e32(120), e32(16), e32(0), e32(0), e32(20), e32(10)]));
    payload.extend(info_header(2, 2, 24, 16));
    payload.extend(dib24_bottom_up().get(40..).unwrap_or_default());
    let efile = emf(
        &[
            erec(59, &[]),
            erec(27, &cat(vec![e32(0), e32(0)])),
            erec(54, &cat(vec![e32(9), e32(9)])),
            erec(61, &[]),
            erec(60, &[]),
            erec(63, &[0; 16]),
            erec(81, &payload),
            erec(86, &cat(vec![h32(4), h16(0), h16(0), h16(9), h16(9), h16(0), h16(9)])),
        ],
        [0, 0, 20, 10],
        [0, 0, 2540, 1270],
    );
    for file in [&wfile, &efile] {
        for n in 0..=file.len() {
            let prefix = &file[..n];
            let _ = parse(prefix);
            let _ = is_metafile(prefix);
        }
    }
    let mut rng = Lcg(0x2545_F491_4F6C_DD1D);
    for _ in 0..3000 {
        let len = (rng.rnd() % 400) as usize;
        let buf: Vec<u8> = (0..len).map(|_| rng.rnd() as u8).collect();
        let _ = parse(&buf);
    }
    for i in 0..1500usize {
        let mut buf = if i.is_multiple_of(2) { efile.clone() } else { wfile.clone() };
        for _ in 0..8 {
            let at = rng.rnd() as usize % buf.len();
            buf[at] = rng.rnd() as u8;
        }
        let _ = parse(&buf);
    }
    for i in 0..500usize {
        let mut buf = if i.is_multiple_of(2) { efile.clone() } else { wfile.clone() };
        let at = rng.rnd() as usize % buf.len();
        buf[at..].iter_mut().for_each(|b| *b = 0xFF);
        let _ = parse(&buf);
    }
}

#[test]
fn wmf_stock_null_brush_by_16_bit_handle_has_no_fill() {
    // SELECTOBJECT with 0x8005 is the NULL_BRUSH stock object (0x8000 | 5), so the rectangle is outline only.
    let recs = [wrec(0x012D, &h16(0x8005u16 as i32)), wrec(0x041B, &cat(vec![h16(70), h16(110), h16(20), h16(10)]))];
    let p = parse(&wmf(&recs, None)).unwrap();
    let (_, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(fill, None);
    assert_eq!(stroke, Some(([0, 0, 0, 255], 0.0)));
}

#[test]
fn emf_frame_comes_from_rclframe_not_device_bounds() {
    // Bounds cover only the left half of the 25 x 12.5 mm frame (device units are 0.04 per 0.01 mm), so a
    // rectangle from x = 0 to 50 px ends at half the picture width.
    let rec = erec(43, &cat(vec![e32(0), e32(0), e32(50), e32(25)]));
    let p = parse(&emf(&[rec], [0, 0, 50, 25], [0, 0, 2500, 1250])).unwrap();
    let w = 2500.0 * 72.0 / 2540.0;
    assert!(close(p.width, w as f32) && close(p.height, (1250.0 * 72.0 / 2540.0) as f32));
    let (segs, ..) = path_segs(&p.items[0]);
    assert!(matches!(segs[1], Seg::Line(x, _) if close(x, (w / 2.0) as f32)), "{segs:?}");
}

#[test]
fn emf_bounds_stand_in_when_the_frame_is_empty() {
    let rec = erec(43, &cat(vec![e32(0), e32(0), e32(50), e32(25)]));
    let p = parse(&emf(&[rec], [0, 0, 50, 25], [0; 4])).unwrap();
    assert_eq!((p.width, p.height), (50.0, 25.0));
}

#[test]
fn wmf_placeable_bounds_go_through_the_window_mapping() {
    // Logical bounds 200 x 100 at 1440 per inch are 10 x 5 points; the window is 2:1 onto the viewport, so
    // the frame is 100 x 50 device units and the rectangle fills it.
    let recs = [
        wrec(0x0103, &h16(8)),
        wrec(0x020C, &cat(vec![h16(100), h16(200)])),
        wrec(0x020E, &cat(vec![h16(50), h16(100)])),
        wrec(0x041B, &cat(vec![h16(100), h16(200), h16(0), h16(0)])),
    ];
    let p = parse(&wmf(&recs, Some([0, 0, 200, 100]))).unwrap();
    assert_eq!((p.width, p.height), (10.0, 5.0));
    let (segs, ..) = path_segs(&p.items[0]);
    assert!(segs.iter().any(|s| matches!(s, Seg::Line(x, y) if close(*x, 10.0) && close(*y, 5.0))), "{segs:?}");
}

#[test]
fn wmf_stretchdib_clipped_source_keeps_its_place() {
    // Source x from -1 for 2 columns: only column 0 is in the bitmap, so it fills the right half of the
    // destination and the picture keeps the original 20-unit width scale.
    let params = cat(vec![h32(0x00CC_0020), h16(0), h16(2), h16(2), h16(0), h16(-1), h16(10), h16(20), h16(0), h16(0), dib24_bottom_up()]);
    let p = parse(&wmf(&[wrec(0x0F43, &params)], Some([0, 0, 20, 10]))).unwrap();
    let Item::Bitmap { rect, width, height, rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    // Placeable units are points: 20 units at 1440 per inch are 1 point, so the right column is 0.5 pt wide.
    assert_eq!(rect, &[0.5, 0.0, 0.5, 0.5]);
    assert_eq!((*width, *height), (1, 2));
    assert_eq!(rgba.as_slice(), [255, 0, 0, 255, 0, 0, 255, 255]);
}

#[test]
fn wmf_negative_destination_width_mirrors_the_bitmap() {
    let params = cat(vec![h32(0x00CC_0020), h16(0), h16(2), h16(2), h16(0), h16(0), h16(10), h16(-20), h16(0), h16(0), dib24_bottom_up()]);
    // Frame from the placeable bounds (-20, 0)-(0, 10): the mirrored bitmap fills it.
    let p = parse(&wmf(&[wrec(0x0F43, &params)], Some([-20, 0, 0, 10]))).unwrap();
    let Item::Bitmap { rect, rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rect, &[0.0, 0.0, 1.0, 0.5]);
    assert_eq!(&rgba[..8], [255, 255, 255, 255, 255, 0, 0, 255]);
}

#[test]
fn emf_blackness_bitblt_fills_black_without_a_source() {
    let mut payload = vec![0u8; 16];
    payload.extend(cat(vec![e32(10), e32(20), e32(30), e32(40), e32(0x0000_0042)]));
    let p = parse(&emf(&[erec(76, &payload)], [0; 4], [0; 4])).unwrap();
    let (segs, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(fill, Some([0, 0, 0, 255]));
    assert_eq!(stroke, None);
    assert_eq!(segs.len(), 5);
}

#[test]
fn emf_savedc_past_the_stack_cap_stops_the_parse() {
    let mut recs: Vec<Vec<u8>> = (0..=crate::MAX_STACK).map(|_| erec(33, &[])).collect();
    recs.push(erec(43, &cat(vec![e32(0), e32(0), e32(10), e32(10)])));
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    assert!(p.items.is_empty(), "records after the cap must not be drawn");
}

#[test]
fn canvas_stops_at_the_byte_budget() {
    // Two 128 MB bitmaps fit the budget; the third would exceed it and stops the canvas.
    let mut cv = Canvas::new();
    let big = || Rgba { w: 1, h: 1, bottom_up: false, px: vec![0u8; MAX_BYTES / 2] };
    cv.bitmap([0.0, 0.0, 1.0, 1.0], big());
    cv.bitmap([0.0, 0.0, 1.0, 1.0], big());
    assert!(!cv.full());
    cv.bitmap([0.0, 0.0, 1.0, 1.0], big());
    assert!(cv.full());
    assert_eq!(cv.items.len(), 2);
}

#[test]
fn oversized_dib_header_size_is_rejected_without_overflow() {
    let mut info = info_header(2, 2, 24, 16);
    info[0..4].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    let params = cat(vec![h32(0x00CC_0020), h16(0), h16(2), h16(2), h16(0), h16(0), h16(10), h16(20), h16(0), h16(0), info]);
    let p = parse(&wmf(&[wrec(0x0F43, &params)], None)).unwrap();
    assert!(p.items.is_empty());
}

/// An EMF record of `len` bytes (header included) with u32 fields and byte blobs written at absolute
/// record offsets, so each test places a field where the spec puts it.
fn erec_at(typ: u32, len: usize, fields: &[(usize, u32)], blobs: &[(usize, &[u8])]) -> Vec<u8> {
    let mut v = vec![0u8; len];
    v[0..4].copy_from_slice(&typ.to_le_bytes());
    v[4..8].copy_from_slice(&(len as u32).to_le_bytes());
    for (off, val) in fields {
        v[*off..*off + 4].copy_from_slice(&val.to_le_bytes());
    }
    for (off, b) in blobs {
        v[*off..*off + b.len()].copy_from_slice(b);
    }
    v
}

/// A BITBLT-style record (BITBLT, STRETCHBLT, ALPHABLEND layout) with its DIB at 112: the bmi and bits
/// offsets and lengths go at 84 and 92. `fields` and `blobs` add the other fields.
fn dib_rec(typ: u32, fields: &[(usize, u32)], blobs: &[(usize, &[u8])], info: &[u8], bits: &[u8]) -> Vec<u8> {
    let at = 112;
    let bits_at = at + info.len();
    let mut f = fields.to_vec();
    f.extend([(84, at as u32), (88, info.len() as u32), (92, bits_at as u32), (96, bits.len() as u32)]);
    let mut b: Vec<(usize, &[u8])> = blobs.to_vec();
    b.extend([(at, info), (bits_at, bits)]);
    erec_at(typ, bits_at + bits.len(), &f, &b)
}

/// BITBLT (76) with destination `dst` = [x, y, w, h] and a 40-byte header DIB.
fn bitblt_rec(rop: u32, dst: [u32; 4], info: &[u8], bits: &[u8]) -> Vec<u8> {
    let fields = [(24, dst[0]), (28, dst[1]), (32, dst[2]), (36, dst[3]), (40, rop)];
    dib_rec(76, &fields, &[], info, bits)
}

#[test]
fn wmf_font_takes_an_object_slot_so_handles_line_up() {
    // The font is handle 0, so the pen created after it is handle 1.
    let recs = [
        wrec(0x02FB, &[0u8; 18]),
        wrec(0x02FA, &cat(vec![h16(0), h16(4), h16(0), h32(0x0000_FF00)])),
        wrec(0x012D, &h16(1)),
        wrec(0x0325, &cat(vec![h16(2), h16(0), h16(0), h16(50), h16(0), h16(50)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    let (_, _, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(stroke, Some(([0, 255, 0, 255], 4.0)));
}

#[test]
fn emf_polybezierto16_continues_the_open_path() {
    // BEGINPATH, MOVETO a, LINETO b, POLYBEZIERTO16, CLOSEFIGURE, FILLPATH: one closed figure from a.
    let mut bezier = vec![0u8; 16];
    bezier.extend(cat(vec![e32(3), h16(10), h16(10), h16(20), h16(10), h16(20), h16(0)]));
    let recs = [
        erec(59, &[]),
        erec(27, &cat(vec![e32(0), e32(0)])),
        erec(54, &cat(vec![e32(10), e32(0)])),
        erec(88, &bezier),
        erec(61, &[]),
        erec(62, &[0; 16]),
    ];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 1);
    let (segs, fill, _, _) = path_segs(&p.items[0]);
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(10.0, 0.0), Seg::Cubic(10.0, 10.0, 20.0, 10.0, 20.0, 0.0), Seg::Close]);
    assert_eq!(fill, Some([255, 255, 255, 255]));
}

#[test]
fn emf_polybezier_starts_its_own_figure_and_keeps_the_current_point() {
    // POLYBEZIER (2) starts at its first point; the LINETO after it still starts at the MOVETO point.
    let mut bezier = vec![0u8; 16];
    bezier.extend(cat(vec![e32(4), e32(0), e32(0), e32(5), e32(5), e32(10), e32(5), e32(10), e32(0)]));
    let recs = [erec(27, &cat(vec![e32(3), e32(3)])), erec(2, &bezier), erec(54, &cat(vec![e32(50), e32(50)]))];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 2);
    let (segs, ..) = path_segs(&p.items[0]);
    assert_eq!(segs[0], Seg::Move(0.0, 0.0));
    let (segs, ..) = path_segs(&p.items[1]);
    assert_eq!(segs, &vec![Seg::Move(3.0, 3.0), Seg::Line(50.0, 50.0)]);
}

#[test]
fn hairline_pen_stays_zero_under_an_anisotropic_window() {
    // Window 2000 x 2000 onto a 1 x 1 viewport: a cosmetic pen must not become one device unit wide.
    let recs = [
        wrec(0x0103, &h16(8)),
        wrec(0x020C, &cat(vec![h16(2000), h16(2000)])),
        wrec(0x020E, &cat(vec![h16(1), h16(1)])),
        wrec(0x0214, &cat(vec![h16(0), h16(0)])),
        wrec(0x0213, &cat(vec![h16(2000), h16(2000)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    assert_eq!((p.width, p.height), (1.0, 1.0));
    let (segs, _, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(segs.last(), Some(&Seg::Line(1.0, 1.0)));
    assert_eq!(stroke, Some(([0, 0, 0, 255], 0.0)));
}

#[test]
fn null_pen_is_recognised_under_extra_style_bits() {
    // Pen style 0x105: PS_NULL (5) with a flag above the style nibble still draws no outline.
    let pen = erec_at(38, 28, &[(8, 0), (12, 0x105), (16, 1), (24, 0)], &[]);
    let recs = [pen, erec(37, &h32(0)), erec(43, &cat(vec![e32(0), e32(0), e32(10), e32(10)]))];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    let (_, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(fill, Some([255, 255, 255, 255]));
    assert_eq!(stroke, None);
}

#[test]
fn metric_map_mode_flips_y_and_scales_to_device_pixels_emf() {
    // LOMETRIC is 0.1 mm per unit; the header maps 4 device pixels to 1 mm. Points go y-up, so the
    // drawing lies below its origin and the picture frame moves up to its top.
    let recs = [erec(17, &h32(2)), erec(27, &cat(vec![e32(0), e32(0)])), erec(54, &cat(vec![e32(100), e32(50)]))];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    let (segs, ..) = path_segs(&p.items[0]);
    assert!(close_seg(&segs[0], Seg::Move(0.0, 20.0)) && close_seg(&segs[1], Seg::Line(40.0, 0.0)), "{segs:?}");
}

#[test]
fn metric_map_mode_uses_96_dpi_for_wmf() {
    let recs = [wrec(0x0103, &h16(2)), wrec(0x0214, &cat(vec![h16(0), h16(0)])), wrec(0x0213, &cat(vec![h16(50), h16(100)]))];
    let p = parse(&wmf(&recs, None)).unwrap();
    let px_mm = 96.0 / 25.4;
    let (segs, ..) = path_segs(&p.items[0]);
    let (x, y) = (100.0 * 0.1 * px_mm, 50.0 * 0.1 * px_mm);
    assert!(close_seg(&segs[0], Seg::Move(0.0, y as f32)) && close_seg(&segs[1], Seg::Line(x as f32, 0.0)), "{segs:?}");
}

/// Segment equality within [`close`] tolerance.
fn close_seg(a: &Seg, b: Seg) -> bool {
    match (*a, b) {
        (Seg::Move(x, y), Seg::Move(u, v)) | (Seg::Line(x, y), Seg::Line(u, v)) => close(x, u) && close(y, v),
        _ => false,
    }
}

#[test]
fn isotropic_mode_uses_one_scale_and_keeps_axis_signs() {
    // Window 100 x 50; viewport 400 x -100: the scale is min(4, 2) = 2 on both axes, y flipped.
    let recs = [
        wrec(0x0103, &h16(7)),
        wrec(0x020C, &cat(vec![h16(50), h16(100)])),
        wrec(0x020E, &cat(vec![h16(-100), h16(400)])),
        wrec(0x0214, &cat(vec![h16(0), h16(0)])),
        wrec(0x0213, &cat(vec![h16(25), h16(50)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    let (segs, ..) = path_segs(&p.items[0]);
    assert!(close_seg(&segs[0], Seg::Move(0.0, 100.0)) && close_seg(&segs[1], Seg::Line(100.0, 50.0)), "{segs:?}");
}

#[test]
fn modify_world_with_an_unknown_mode_keeps_the_transform() {
    let scale2 = cat([2.0f32, 0.0, 0.0, 2.0, 0.0, 0.0].iter().map(|v| v.to_le_bytes().to_vec()).collect());
    let identity = cat([1.0f32, 0.0, 0.0, 1.0, 0.0, 0.0].iter().map(|v| v.to_le_bytes().to_vec()).collect());
    let recs =
        [erec(35, &scale2), erec(36, &cat(vec![identity, h32(99)])), erec(27, &cat(vec![e32(0), e32(0)])), erec(54, &cat(vec![e32(10), e32(5)]))];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    let (segs, ..) = path_segs(&p.items[0]);
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(20.0, 10.0)]);
}

#[test]
fn round_rect_accepts_reversed_corners() {
    let normal = crate::geom::round_rect(0.0, 0.0, 10.0, 20.0, 4.0, 4.0);
    assert_eq!(crate::geom::round_rect(10.0, 20.0, 0.0, 0.0, 4.0, 4.0), normal);
    assert_eq!(crate::geom::round_rect(10.0, 0.0, 0.0, 20.0, 4.0, 4.0), normal);
}

#[test]
fn stroke_width_is_clamped_to_the_picture_diagonal() {
    // A 1440-unit placeable picture is 72 points square (diagonal 101.8); a 5000-unit pen would be 250.
    let recs = [
        wrec(0x02FA, &cat(vec![h16(0), h16(5000), h16(0), h32(0)])),
        wrec(0x012D, &h16(0)),
        wrec(0x041B, &cat(vec![h16(1440), h16(1440), h16(0), h16(0)])),
    ];
    let p = parse(&wmf(&recs, Some([0, 0, 1440, 1440]))).unwrap();
    assert_eq!((p.width, p.height), (72.0, 72.0));
    let (_, _, stroke, _) = path_segs(&p.items[0]);
    assert!(close(stroke.unwrap_or(([0; 4], 0.0)).1, 72.0 * 2.0f32.sqrt()), "{stroke:?}");
}

/// A WMF rectangle from (0, 0) to (1, 1), so a picture's frame starts at the origin.
fn origin_rect() -> Vec<u8> {
    wrec(0x041B, &cat(vec![h16(1), h16(1), h16(0), h16(0)]))
}

#[test]
fn wmf_dibbitblt_without_a_dib_is_a_pattern_fill() {
    // The 12-word form: rop(4), ySrc, xSrc, reserved, height, width, yDest, xDest; PATCOPY fills with the brush.
    let params = cat(vec![h32(0x00F0_0021), h16(0), h16(0), h16(0), h16(10), h16(20), h16(5), h16(7)]);
    let p = parse(&wmf(&[origin_rect(), wrec(0x0940, &params)], None)).unwrap();
    let (segs, fill, _, _) = path_segs(&p.items[1]);
    assert_eq!(fill, Some([255, 255, 255, 255]));
    assert_eq!(segs, &vec![Seg::Move(7.0, 5.0), Seg::Line(27.0, 5.0), Seg::Line(27.0, 15.0), Seg::Line(7.0, 15.0), Seg::Close]);
}

#[test]
fn wmf_dibstretchblt_without_a_dib_is_a_rop_fill() {
    // The 14-word form: rop(4), srcH, srcW, ySrc, xSrc, reserved, destH, destW, yDest, xDest; BLACKNESS fills black.
    let params = cat(vec![h32(0x0000_0042), h16(2), h16(2), h16(0), h16(0), h16(0), h16(10), h16(20), h16(5), h16(7)]);
    let p = parse(&wmf(&[origin_rect(), wrec(0x0B41, &params)], None)).unwrap();
    let (segs, fill, stroke, _) = path_segs(&p.items[1]);
    assert_eq!((fill, stroke), (Some([0, 0, 0, 255]), None));
    assert_eq!(segs, &vec![Seg::Move(7.0, 5.0), Seg::Line(27.0, 5.0), Seg::Line(27.0, 15.0), Seg::Line(7.0, 15.0), Seg::Close]);
}

#[test]
fn wmf_dibstretchblt_stretches_the_dib_into_its_destination() {
    // rop(4), srcH, srcW, ySrc, xSrc, destH, destW, yDest, xDest, then the DIB (no reserved word).
    let mut params = cat(vec![h32(0x00CC_0020), h16(2), h16(2), h16(0), h16(0), h16(10), h16(20), h16(6), h16(5)]);
    params.extend(dib24_bottom_up());
    let p = parse(&wmf(&[origin_rect(), wrec(0x0B41, &params)], None)).unwrap();
    let Item::Bitmap { rect, rgba, .. } = &p.items[1] else { panic!("expected a bitmap") };
    assert_eq!(rect, &[5.0, 6.0, 20.0, 10.0]);
    assert_eq!(rgba.as_slice(), DIB24_RGBA);
}

#[test]
fn wmf_negative_destination_height_mirrors_the_rows() {
    let params = cat(vec![h32(0x00CC_0020), h16(0), h16(2), h16(2), h16(0), h16(0), h16(-10), h16(20), h16(0), h16(0), dib24_bottom_up()]);
    let p = parse(&wmf(&[wrec(0x0F43, &params)], None)).unwrap();
    let Item::Bitmap { rect, rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rect, &[0.0, 0.0, 20.0, 10.0]);
    // Blue, green on top; red, white below.
    assert_eq!(rgba.as_slice(), [0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 255, 255, 255]);
}

#[test]
fn wmf_dibbitblt_copies_the_dib_at_its_destination() {
    // The rectangle fixes the picture origin at (0, 0); the DIB lands at x = 5, y = 6.
    let mut params = cat(vec![h32(0x00CC_0020), h16(0), h16(0), h16(2), h16(2), h16(6), h16(5)]);
    params.extend(dib24_bottom_up());
    let recs = [wrec(0x041B, &cat(vec![h16(10), h16(10), h16(0), h16(0)])), wrec(0x0940, &params)];
    let p = parse(&wmf(&recs, None)).unwrap();
    let Item::Bitmap { rect, width, height, rgba, .. } = &p.items[1] else { panic!("expected a bitmap") };
    assert_eq!(rect, &[5.0, 6.0, 2.0, 2.0]);
    assert_eq!((*width, *height), (2, 2));
    assert_eq!(rgba.as_slice(), DIB24_RGBA);
}

#[test]
fn emf_setdibitstodevice_draws_the_image_at_its_own_size() {
    let dib = dib24_bottom_up();
    let (info, bits) = (&dib[..40], &dib[40..]);
    // Destination x/y at 24, src x/y at 32, size at 40, bmi at 48 (offset, length), bits at 56, scan start
    // 68 and scan count 72.
    let fields = [(24, 10), (28, 20), (40, 2), (44, 2), (48, 112), (52, 40), (56, 152), (60, 16), (72, 2)];
    let rec = erec_at(80, 168, &fields, &[(112, info), (152, bits)]);
    let p = parse(&emf(&[rec], [0; 4], [0; 4])).unwrap();
    let Item::Bitmap { rect, rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rect, &[0.0, 0.0, 2.0, 2.0]);
    assert_eq!(rgba.as_slice(), DIB24_RGBA);
}

#[test]
fn emf_setdibitstodevice_source_counts_from_the_start_scan() {
    // ySrc 1 with the bits starting at scan 1 is source row 0 of the bits: the bottom row (blue, green),
    // drawn cxSrc x cySrc = 2 x 1 at the destination.
    let dib = dib24_bottom_up();
    let (info, bits) = (&dib[..40], &dib[40..]);
    let fields = [(24, 10), (28, 20), (36, 1), (40, 2), (44, 1), (48, 112), (52, 40), (56, 152), (60, 16), (68, 1), (72, 2)];
    let rec = erec_at(80, 168, &fields, &[(112, info), (152, bits)]);
    let r = erec(43, &cat(vec![e32(0), e32(0), e32(1), e32(1)]));
    let p = parse(&emf(&[r, rec], [0; 4], [0; 4])).unwrap();
    let Item::Bitmap { rect, rgba, .. } = &p.items[1] else { panic!("expected a bitmap") };
    assert_eq!(rect, &[10.0, 20.0, 2.0, 1.0]);
    assert_eq!(rgba.as_slice(), [0, 0, 255, 255, 0, 255, 0, 255]);
}

#[test]
fn emf_alphablend_uses_constant_and_per_pixel_alpha() {
    // Constant alpha 128 (BLENDFUNCTION bytes at 40: op, flags, constant, format) over a 24-bit DIB.
    let blend = [0u8, 0, 128, 0];
    let dib = dib24_bottom_up();
    let rec = dib_rec(114, &[(24, 0), (28, 0), (32, 2), (36, 2), (44, 0), (48, 0), (100, 2), (104, 2)], &[(40, &blend[..])], &dib[..40], &dib[40..]);
    let p = parse(&emf(&[rec], [0; 4], [0; 4])).unwrap();
    let Item::Bitmap { rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    let alpha: Vec<u8> = rgba.as_chunks::<4>().0.iter().map(|px| px[3]).collect();
    assert_eq!(alpha, vec![128; 4]);
    // Per-pixel alpha (AC_SRC_ALPHA) over a 32-bit premultiplied DIB: BGRA 0, 0, 128, 128 is straight red at
    // half alpha, and the constant 128 halves the alpha again.
    let one_px = |constant: u8, bgra: [u8; 4]| {
        let blend = [0u8, 0, constant, 1];
        let info = info_header(1, 1, 32, 4);
        let rec = dib_rec(114, &[(24, 0), (28, 0), (32, 1), (36, 1), (44, 0), (48, 0), (100, 1), (104, 1)], &[(40, &blend[..])], &info, &bgra);
        let p = parse(&emf(&[rec], [0; 4], [0; 4])).unwrap();
        let Item::Bitmap { rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
        rgba.clone()
    };
    assert_eq!(one_px(255, [0, 0, 128, 128]), [255, 0, 0, 128]);
    assert_eq!(one_px(128, [0, 0, 128, 128]), [255, 0, 0, 64]);
    assert_eq!(one_px(255, [10, 20, 64, 128]), [128, 40, 20, 128], "rounded to the nearest value");
}

#[test]
fn emf_srcand_mask_draws_all_but_its_white_pixels() {
    // SRCAND over white paper is the source itself; white keeps the destination, so it is transparent.
    let dib = dib24_bottom_up();
    let rec = bitblt_rec(0x0088_00C6, [0, 0, 2, 2], &dib[..40], &dib[40..]);
    let p = parse(&emf(&[rec], [0; 4], [0; 4])).unwrap();
    let Item::Bitmap { rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rgba.as_slice(), [255, 0, 0, 255, 255, 255, 255, 0, 0, 0, 255, 255, 0, 255, 0, 255]);
}

#[test]
fn emf_srcpaint_makes_black_pixels_transparent() {
    // A 2 x 1 DIB: black, then red. SRCPAINT leaves black transparent; SRCCOPY keeps it opaque.
    let info = info_header(2, 1, 24, 8);
    let bits = [0u8, 0, 0, 0, 0, 255, 0, 0];
    let p = parse(&emf(&[bitblt_rec(0x00EE_0086, [0, 0, 2, 1], &info, &bits)], [0; 4], [0; 4])).unwrap();
    let Item::Bitmap { rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rgba.as_slice(), [0, 0, 0, 0, 255, 0, 0, 255]);
    let p = parse(&emf(&[bitblt_rec(0x00CC_0020, [0, 0, 2, 1], &info, &bits)], [0; 4], [0; 4])).unwrap();
    let Item::Bitmap { rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rgba.as_slice(), [0, 0, 0, 255, 255, 0, 0, 255]);
}

#[test]
fn wmf_dib_palette_entries_are_skipped_before_the_bits() {
    // A 24-bit DIB with biClrUsed = 2 carries two RGBQUADs (8 bytes) before its pixel bits.
    let mut info = info_header(2, 2, 24, 16);
    info[32..36].copy_from_slice(&2u32.to_le_bytes());
    let mut dib = info;
    dib.extend([9u8; 8]);
    dib.extend(&dib24_bottom_up()[40..]);
    let params = cat(vec![h32(0x00CC_0020), h16(0), h16(2), h16(2), h16(0), h16(0), h16(10), h16(20), h16(0), h16(0), dib]);
    let p = parse(&wmf(&[wrec(0x0F43, &params)], None)).unwrap();
    let Item::Bitmap { rgba, .. } = &p.items[0] else { panic!("expected a bitmap") };
    assert_eq!(rgba.as_slice(), DIB24_RGBA);
}

#[test]
fn emf_point_counts_past_the_budget_are_rejected() {
    // A count of 4 billion points in a record that cannot hold them: nothing is drawn and nothing is allocated.
    let mut poly = vec![0u8; 16];
    poly.extend(cat(vec![h32(u32::MAX), h32(0), h32(0)]));
    let mut groups = vec![0u8; 16];
    groups.extend(cat(vec![h32(u32::MAX), h32(u32::MAX), h32(1)]));
    let p = parse(&emf(&[erec(4, &poly), erec(7, &groups)], [0; 4], [0; 4])).unwrap();
    assert!(p.items.is_empty());
}

#[test]
fn emf_polypolygon_counts_must_fit_the_record() {
    // A polygon count within the point budget in a record too short to hold the counts draws nothing.
    let mut short = vec![0u8; 16];
    short.extend(cat(vec![h32(crate::MAX_PATH_OPS as u32), h32(3), h32(3)]));
    // POLYPOLYGON16: two triangles of three points each.
    let mut two = vec![0u8; 16];
    two.extend(cat(vec![h32(2), h32(6), h32(3), h32(3)]));
    two.extend(cat([0, 0, 10, 0, 0, 10, 20, 0, 30, 0, 20, 10].iter().map(|v| h16(*v)).collect()));
    let p = parse(&emf(&[erec(91, &short), erec(91, &two)], [0; 4], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 1);
    let (segs, ..) = path_segs(&p.items[0]);
    assert_eq!(segs.iter().filter(|s| matches!(s, Seg::Move(..))).count(), 2, "{segs:?}");
}

/// EMR_EXTCREATEPEN into slot 1 with LogPenEx style, width and brush style, then a selected rectangle.
fn ext_pen_rect(style: u32, width: u32, brush: u32) -> Option<([u8; 4], f32)> {
    let pen = erec_at(95, 52, &[(8, 1), (28, style), (32, width), (36, brush), (40, 0x0000_FF00)], &[]);
    let recs = [pen, erec(37, &h32(1)), erec(43, &cat(vec![e32(0), e32(0), e32(10), e32(10)]))];
    let p = parse(&emf(&recs, [0; 4], [0; 4])).unwrap();
    path_segs(&p.items[0]).2
}

#[test]
fn emf_ext_pen_cosmetic_is_a_hairline_and_null_brush_draws_nothing() {
    // PS_COSMETIC widths are device pixels: a hairline, not one logical unit.
    assert_eq!(ext_pen_rect(0, 1, 0), Some(([0, 255, 0, 255], 0.0)));
    // PS_GEOMETRIC (0x10000) keeps its logical width.
    assert_eq!(ext_pen_rect(0x0001_0000, 4, 0), Some(([0, 255, 0, 255], 4.0)));
    // A BS_NULL brush draws no line.
    assert_eq!(ext_pen_rect(0x0001_0000, 4, 1), None);
}

#[test]
fn flip_tolerates_a_short_pixel_buffer() {
    let mut img = Rgba { w: 2, h: 2, bottom_up: false, px: vec![1u8; 7] };
    crate::dib::flip(&mut img, true, true);
    assert_eq!(img.px.len(), 7);
    // Three rows held of four: only the rows present swap (0 with 2).
    let mut img = Rgba { w: 1, h: 4, bottom_up: false, px: (0u8..12).collect() };
    crate::dib::flip(&mut img, false, true);
    assert_eq!(img.px, [8, 9, 10, 11, 4, 5, 6, 7, 0, 1, 2, 3]);
}

#[test]
fn place_fits_the_frame_scales_strokes_and_drops_far_paths() {
    let pic = crate::Picture {
        width: 100.0,
        height: 50.0,
        items: vec![
            Item::Path {
                segs: vec![Seg::Move(0.0, 0.0), Seg::Line(10.0, 10.0)],
                fill: None,
                stroke: Some(([0, 0, 0, 255], 4.0)),
                even_odd: true,
                region: None,
            },
            Item::Path { segs: vec![Seg::Move(0.0, 0.0), Seg::Line(2e6, 0.0)], fill: None, stroke: None, even_odd: true, region: None },
        ],
    };
    let placed = pic.place(kurbo::Rect::new(0.0, 0.0, 200.0, 100.0), [0.0; 4]);
    assert_eq!(placed.len(), 1);
    let crate::PlacedItem::Path { stroke, .. } = &placed[0] else { panic!("expected a path") };
    assert_eq!(stroke.map(|s| s.1), Some(8.0));
}

#[test]
fn place_keeps_only_finite_bounded_bitmaps_with_area() {
    let bitmap = |rect: [f32; 4]| Item::Bitmap { rect, width: 1, height: 1, rgba: vec![0; 4], region: None };
    let pic = crate::Picture {
        width: 100.0,
        height: 50.0,
        items: vec![
            bitmap([f32::NAN, 0.0, 10.0, 10.0]),
            bitmap([0.0, 0.0, 0.0, 10.0]),
            bitmap([0.0, 0.0, 2e6, 10.0]),
            bitmap([0.0, 0.0, f32::INFINITY, 10.0]),
            bitmap([10.0, 5.0, 10.0, 10.0]),
        ],
    };
    let placed = pic.place(kurbo::Rect::new(0.0, 0.0, 200.0, 100.0), [0.0; 4]);
    assert_eq!(placed.len(), 1);
    let crate::PlacedItem::Bitmap { index, rect, .. } = &placed[0] else { panic!("expected a bitmap") };
    assert_eq!((*index, *rect), (4, kurbo::Rect::new(20.0, 10.0, 40.0, 30.0)));
}

#[test]
fn take_pixels_returns_only_drawable_bitmaps() {
    let mut ok = Item::Bitmap { rect: [0.0; 4], width: 2, height: 1, rgba: vec![7; 8], region: None };
    assert_eq!(ok.take_pixels(), Some((2, 1, vec![7; 8])));
    assert!(matches!(&ok, Item::Bitmap { rgba, .. } if rgba.is_empty()), "the pixels move out");
    let mut short = Item::Bitmap { rect: [0.0; 4], width: 2, height: 2, rgba: vec![0; 8], region: None };
    assert_eq!(short.take_pixels(), None);
    let side = crate::MAX_BITMAP_SIDE + 1;
    let mut wide = Item::Bitmap { rect: [0.0; 4], width: side, height: 1, rgba: vec![0; side as usize * 4], region: None };
    assert_eq!(wide.take_pixels(), None);
    let mut path = Item::Path { segs: Vec::new(), fill: None, stroke: None, even_odd: false, region: None };
    assert_eq!(path.take_pixels(), None);
    let pic = |width: f32, height: f32| crate::Picture { width, height, items: Vec::new() };
    assert!(pic(1.0, 2.0).has_size());
    assert!(!pic(0.0, 2.0).has_size() && !pic(f32::NAN, 2.0).has_size() && !pic(1.0, f32::INFINITY).has_size());
}

/// The rectangle a WMF RECTANGLE record covers, from its path's corners.
fn rect_extent(item: &Item) -> (f32, f32, f32, f32) {
    let (segs, ..) = path_segs(item);
    let pts: Vec<(f32, f32)> = segs
        .iter()
        .filter_map(|s| match s {
            Seg::Move(x, y) | Seg::Line(x, y) => Some((*x, *y)),
            _ => None,
        })
        .collect();
    let (xs, ys): (Vec<f32>, Vec<f32>) = pts.into_iter().unzip();
    let lo = |v: &[f32]| v.iter().copied().fold(f32::MAX, f32::min);
    let hi = |v: &[f32]| v.iter().copied().fold(f32::MIN, f32::max);
    (lo(&xs), lo(&ys), hi(&xs), hi(&ys))
}

#[test]
fn plain_anisotropic_wmf_without_a_viewport_fills_its_window() {
    // Anisotropic, window 200 x 100, no viewport (a logo strip saved without a placeable header): the
    // window is the target, not a 1 x 1 viewport that squeezes the picture to a point.
    let recs = [
        wrec(0x0103, &h16(8)),
        wrec(0x020B, &cat(vec![h16(0), h16(0)])),
        wrec(0x020C, &cat(vec![h16(100), h16(200)])),
        wrec(0x041B, &cat(vec![h16(100), h16(200), h16(0), h16(0)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    assert_eq!((p.width, p.height), (200.0, 100.0));
    let (x0, y0, x1, y1) = rect_extent(&p.items[0]);
    assert!(close(x0, 0.0) && close(y0, 0.0) && close(x1, 200.0) && close(y1, 100.0), "{x0} {y0} {x1} {y1}");
}

#[test]
fn placeable_wmf_window_records_replace_the_bounds() {
    // The placeable bounds say 1440 square (72 points); the file then draws in a window centred on the
    // origin, as a coat of arms saved by a drawing program does. The window maps onto the 72-point
    // target, so the drawing fills it instead of being blown up around the bounds.
    let recs = [
        wrec(0x020C, &cat(vec![h16(2000), h16(2000)])),
        wrec(0x020B, &cat(vec![h16(-1000), h16(-1000)])),
        wrec(0x041B, &cat(vec![h16(1000), h16(1000), h16(-1000), h16(-1000)])),
    ];
    let p = parse(&wmf(&recs, Some([238, 539, 1678, 1979]))).unwrap();
    assert_eq!((p.width, p.height), (72.0, 72.0));
    let (x0, y0, x1, y1) = rect_extent(&p.items[0]);
    assert!(close(x0, 0.0) && close(y0, 0.0) && close(x1, 72.0) && close(y1, 72.0), "{x0} {y0} {x1} {y1}");
}

#[test]
fn placeable_wmf_keeps_its_target_when_the_window_changes_shape() {
    // Bounds 600 x 300 (30 x 15 points), then a portrait window 100 x 200 for a bitmap-sized drawing:
    // the drawing still fills the target (Word stretches it to the picture's box).
    let recs = [wrec(0x0103, &h16(8)), wrec(0x020C, &cat(vec![h16(200), h16(100)])), wrec(0x041B, &cat(vec![h16(200), h16(100), h16(0), h16(0)]))];
    let p = parse(&wmf(&recs, Some([0, 0, 600, 300]))).unwrap();
    assert_eq!((p.width, p.height), (30.0, 15.0));
    let (x0, y0, x1, y1) = rect_extent(&p.items[0]);
    assert!(close(x0, 0.0) && close(y0, 0.0) && close(x1, 30.0) && close(y1, 15.0), "{x0} {y0} {x1} {y1}");
}

#[test]
fn rop2_nop_draws_nothing_until_the_mode_changes() {
    // A coat of arms that first draws its bounding rectangle with the R2_NOP mix mode (no output),
    // then its shapes with R2_COPYPEN. Restoring a saved state brings R2_NOP back.
    let rect = |n: i32| wrec(0x041B, &cat(vec![h16(n), h16(n), h16(0), h16(0)]));
    let recs = [wrec(0x0104, &h16(11)), rect(100), wrec(0x001E, &[]), wrec(0x0104, &h16(13)), rect(50), wrec(0x0127, &h16(-1)), rect(20)];
    let p = parse(&wmf(&recs, Some([0, 0, 100, 100]))).unwrap();
    assert_eq!(p.items.len(), 1, "{:?}", p.items);
    let (_, _, x1, y1) = rect_extent(&p.items[0]);
    // 50 of 100 units in a 5-point picture.
    assert!(close(x1, 2.5) && close(y1, 2.5), "{x1} {y1}");
}

#[test]
fn plain_wmf_target_comes_from_a_window_that_was_read() {
    // A window record too short to read, then the real one: the target is the real window, not the
    // 1 x 1 default the short record left in place. A viewport origin set before it is kept.
    let recs = [
        wrec(0x0103, &h16(8)),
        wrec(0x020D, &cat(vec![h16(0), h16(0)])),
        wrec(0x020C, &[]),
        wrec(0x020C, &cat(vec![h16(100), h16(200)])),
        wrec(0x041B, &cat(vec![h16(100), h16(200), h16(0), h16(0)])),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    assert_eq!((p.width, p.height), (200.0, 100.0));
    let (x0, y0, x1, y1) = rect_extent(&p.items[0]);
    assert!(close(x0, 0.0) && close(y0, 0.0) && close(x1, 200.0) && close(y1, 100.0), "{x0} {y0} {x1} {y1}");
}

// ---- Text, pattern and hatch brushes, mix-mode idioms ----

use crate::{HAlign, Picture, Text, VAlign};

/// META_CREATEFONTINDIRECT: height, width, escapement (and orientation), weight, the italic,
/// underline, strike-out and charset bytes, four precision bytes, then the NUL-terminated face.
fn wfont(height: i32, weight: i32, flags: [u8; 3], charset: u8, esc: i32, face: &[u8]) -> Vec<u8> {
    let mut v = cat(vec![h16(height), h16(0), h16(esc), h16(esc), h16(weight)]);
    v.extend(flags);
    v.extend([charset, 0, 0, 0, 0]);
    v.extend(face);
    v.push(0);
    wrec(0x02FB, &v)
}

/// META_EXTTEXTOUT at (x, y): the rectangle when given, the string padded to a word, then `dx`.
fn wtext(x: i32, y: i32, s: &[u8], opts: i32, rect: Option<[i32; 4]>, dx: &[i32]) -> Vec<u8> {
    let mut v = cat(vec![h16(y), h16(x), h16(s.len() as i32), h16(opts)]);
    for c in rect.into_iter().flatten() {
        v.extend(h16(c));
    }
    v.extend(s);
    if s.len() % 2 == 1 {
        v.push(0);
    }
    for d in dx {
        v.extend(h16(*d));
    }
    wrec(0x0A32, &v)
}

/// A plain WMF whose 200 × 100 window maps one logical unit to one picture unit.
fn wmf200(records: &[Vec<u8>]) -> Picture {
    let mut recs = vec![wrec(0x020C, &cat(vec![h16(100), h16(200)]))];
    recs.extend(records.iter().cloned());
    parse(&wmf(&recs, None)).unwrap()
}

fn texts(p: &Picture) -> Vec<&Text> {
    p.items
        .iter()
        .filter_map(|i| match i {
            Item::Text(t) => Some(t.as_ref()),
            _ => None,
        })
        .collect()
}

fn close6(a: [f64; 6], b: [f64; 6]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

#[test]
fn wmf_text_reads_the_font_string_dx_colour_and_alignment() {
    let recs = [
        wfont(-20, 700, [1, 1, 1], 0, 0, b"Arial"),
        wrec(0x012D, &h16(0)),
        wrec(0x0209, &h32(0x0000_00FF)),
        wrec(0x0102, &h16(1)),
        wrec(0x012E, &h16(24 | 6)),
        wtext(50, 60, b"Hi!", 0, None, &[10, 8, 6]),
    ];
    let p = wmf200(&recs);
    let t = texts(&p);
    assert_eq!(t.len(), 1, "{:?}", p.items);
    let t = t[0];
    assert_eq!(t.text, "Hi!");
    assert_eq!(t.dx, vec![10.0, 8.0, 6.0], "the dx array follows the word-padded odd string");
    assert_eq!(t.font.family, "Arial");
    assert_eq!((t.font.height, t.font.weight, t.font.italic, t.font.underline, t.font.strike), (-20.0, 700, true, true, true));
    assert_eq!(t.color, [255, 0, 0, 255]);
    assert_eq!(t.background, None, "TRANSPARENT background mode");
    assert_eq!((t.h_align, t.v_align), (HAlign::Center, VAlign::Baseline));
    assert!(close6(t.transform, [1.0, 0.0, 0.0, 1.0, 50.0, 60.0]), "{:?}", t.transform);
    assert_eq!(t.clip, None);
}

#[test]
fn font_height_sign_and_default_state_are_kept() {
    // A positive height is the cell height; it stays positive for the renderer to resolve. Without
    // SETTEXTALIGN the reference point is the top left; without SETBKMODE the cell is filled white.
    let recs = [wfont(30, 400, [0, 0, 0], 0, 0, b"Times New Roman"), wrec(0x012D, &h16(0)), wtext(5, 5, b"x", 0, None, &[])];
    let p = wmf200(&recs);
    let t = texts(&p)[0];
    assert_eq!(t.font.height, 30.0);
    assert_eq!(t.font.family, "Times New Roman");
    assert_eq!((t.h_align, t.v_align), (HAlign::Left, VAlign::Top));
    assert_eq!(t.background, Some([255, 255, 255, 255]));
    assert!(t.dx.is_empty(), "no dx array in the record");
    // Right and bottom.
    let p = wmf200(&[wrec(0x012E, &h16(2 | 8)), wtext(5, 5, b"x", 0, None, &[])]);
    let t = texts(&p)[0];
    assert_eq!((t.h_align, t.v_align), (HAlign::Right, VAlign::Bottom));
    assert_eq!(t.font, crate::Font::default(), "no font selected");
}

#[test]
fn escapement_turns_the_baseline_counter_clockwise() {
    let recs = [wfont(-20, 400, [0, 0, 0], 0, 900, b"Arial"), wrec(0x012D, &h16(0)), wtext(100, 90, b"Up", 0, None, &[])];
    let p = wmf200(&recs);
    let t = texts(&p)[0];
    // Text x runs up the picture (0, -1); text y (down the glyphs) runs right (1, 0).
    assert!(close6(t.transform, [0.0, -1.0, 1.0, 0.0, 100.0, 90.0]), "{:?}", t.transform);
}

#[test]
fn a_y_up_mapping_moves_text_but_does_not_mirror_it() {
    // Window 200 × -100 onto a 200 × 100 viewport: y goes up.
    let recs = [
        wrec(0x0103, &h16(8)),
        wrec(0x020C, &cat(vec![h16(-100), h16(200)])),
        wrec(0x020E, &cat(vec![h16(100), h16(200)])),
        wtext(40, -30, b"A", 0, None, &[]),
    ];
    let p = parse(&wmf(&recs, None)).unwrap();
    let t = texts(&p)[0];
    assert!(close6(t.transform, [1.0, 0.0, 0.0, 1.0, 40.0, 30.0]), "{:?}", t.transform);
}

#[test]
fn update_cp_starts_at_the_current_position_and_moves_past_the_string() {
    let recs =
        [wrec(0x012E, &h16(1)), wrec(0x0214, &cat(vec![h16(20), h16(10)])), wtext(0, 0, b"ab", 0, None, &[5, 7]), wtext(0, 0, b"c", 0, None, &[3])];
    let p = wmf200(&recs);
    let t = texts(&p);
    assert_eq!(t.len(), 2);
    assert!(close6(t[0].transform, [1.0, 0.0, 0.0, 1.0, 10.0, 20.0]), "{:?}", t[0].transform);
    assert!(close6(t[1].transform, [1.0, 0.0, 0.0, 1.0, 22.0, 20.0]), "{:?}", t[1].transform);
}

#[test]
fn eto_opaque_fills_the_rectangle_and_eto_clipped_clips() {
    let recs = [wrec(0x0201, &h32(0x0000_FF00)), wtext(12, 30, b"ok", 2 | 4, Some([10, 10, 60, 40]), &[])];
    let p = wmf200(&recs);
    assert_eq!(p.items.len(), 2, "{:?}", p.items);
    let (x0, y0, x1, y1) = rect_extent(&p.items[0]);
    assert!(close(x0, 10.0) && close(y0, 10.0) && close(x1, 60.0) && close(y1, 40.0));
    assert_eq!(path_segs(&p.items[0]).1, Some([0, 255, 0, 255]));
    let t = texts(&p)[0];
    assert_eq!(t.clip, Some([10.0, 10.0, 60.0, 40.0]));
    assert_eq!(t.background, Some([0, 255, 0, 255]));
}

#[test]
fn character_sets_decode_bytes() {
    let one = |charset: u8, s: &[u8]| {
        let p = wmf200(&[wfont(-10, 400, [0, 0, 0], charset, 0, b"F"), wrec(0x012D, &h16(0)), wtext(0, 0, s, 0, None, &[])]);
        texts(&p)[0].text.clone()
    };
    assert_eq!(one(0, &[b'A', 0x80, 0x93, 0x94, 0xE9]), "A€“”é");
    assert_eq!(one(238, &[0xB9, 0xE8, 0x9E, 0xA3]), "ąčžŁ");
    assert_eq!(one(204, &[0xC0, 0xFF, 0xA8]), "АяЁ");
    assert_eq!(one(2, b"n\xA7"), "\u{F06E}\u{F0A7}", "symbol fonts keep their codes in the private-use area");
    assert_eq!(one(0, &[0x81]), "\u{FFFD}", "an undefined byte");
    // A double-byte character set: a lead and trail byte become one blank with both advances.
    let p =
        wmf200(&[wfont(-10, 400, [0, 0, 0], 128, 0, b"F"), wrec(0x012D, &h16(0)), wtext(0, 0, &[0x41, 0x82, 0xA0, 0x42], 0, None, &[1, 2, 3, 4])]);
    let t = texts(&p)[0];
    assert_eq!((t.text.as_str(), t.dx.as_slice()), ("A B", [1.0, 5.0, 4.0].as_slice()));
}

#[test]
fn wmf_textout_reads_its_string_and_position() {
    let mut v = cat(vec![h16(3)]);
    v.extend(b"abc\0");
    v.extend(cat(vec![h16(40), h16(30)]));
    let p = wmf200(&[wrec(0x0521, &v)]);
    let t = texts(&p)[0];
    assert_eq!(t.text, "abc");
    assert!(close6(t.transform, [1.0, 0.0, 0.0, 1.0, 30.0, 40.0]), "{:?}", t.transform);
}

#[test]
fn savedc_restores_text_colour_and_font() {
    let recs = [
        wfont(-10, 400, [0, 0, 0], 0, 0, b"One"),
        wrec(0x012D, &h16(0)),
        wrec(0x0209, &h32(0x0000_00FF)),
        wrec(0x001E, &[]),
        wfont(-10, 400, [0, 0, 0], 0, 0, b"Two"),
        wrec(0x012D, &h16(1)),
        wrec(0x0209, &h32(0x00FF_0000)),
        wrec(0x0127, &h16(-1)),
        wtext(0, 0, b"x", 0, None, &[]),
    ];
    let p = wmf200(&recs);
    let t = texts(&p)[0];
    assert_eq!((t.font.family.as_str(), t.color), ("One", [255, 0, 0, 255]));
}

/// EMR_EXTCREATEFONTINDIRECTW in slot 1: LogFont at 12, face at 40.
fn efont(height: i32, esc: i32, weight: i32, italic: u8, charset: u8, face: &str) -> Vec<u8> {
    let mut v = cat(vec![h32(1), e32(height), e32(0), e32(esc), e32(esc), e32(weight)]);
    v.extend([italic, 0, 0, charset, 0, 0, 0, 0]);
    let mut name: Vec<u8> = face.encode_utf16().flat_map(u16::to_le_bytes).collect();
    name.resize(64, 0);
    v.extend(name);
    erec(82, &v)
}

/// EMR_EXTTEXTOUTW (or A) at (x, y) with options, the rectangle (absent under ETO_NO_RECT), the
/// string at 76 and the dx array after it.
fn etext(wide: bool, x: i32, y: i32, units: &[u16], opts: u32, dx: &[i32]) -> Vec<u8> {
    let no_rect = opts & 0x100 != 0;
    let fixed = if no_rect { 60 } else { 76 };
    let s: Vec<u8> = if wide { units.iter().flat_map(|u| u.to_le_bytes()).collect() } else { units.iter().map(|u| *u as u8).collect() };
    let s_off = fixed;
    let dx_off = (s_off + s.len()).next_multiple_of(4);
    let len = dx_off + dx.len() * 4;
    let mut f = vec![(24, 1u32), (36, x as u32), (40, y as u32), (44, units.len() as u32), (48, s_off as u32), (52, opts)];
    f.push((if no_rect { 56 } else { 72 }, if dx.is_empty() { 0 } else { dx_off as u32 }));
    let dxb: Vec<u8> = dx.iter().flat_map(|d| d.to_le_bytes()).collect();
    erec_at(if wide { 84 } else { 83 }, len.next_multiple_of(4), &f, &[(s_off, &s), (dx_off, &dxb)])
}

#[test]
fn emf_exttextoutw_reads_utf16_dx_and_the_font() {
    let recs = [
        efont(24, 0, 400, 1, 0, "Segoe UI"),
        erec(37, &h32(1)),
        erec(22, &h32(0x18 | 2)),
        erec(24, &h32(0x0000_FF00)),
        erec(18, &h32(1)),
        etext(true, 100, 50, &[0x41, 0xD83D, 0xDE00], 0, &[10, 6, 0]),
    ];
    let p = parse(&emf(&recs, [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    let t = texts(&p);
    assert_eq!(t.len(), 1, "{:?}", p.items);
    let t = t[0];
    assert_eq!(t.text, "A\u{1F600}");
    assert_eq!(t.dx, vec![10.0, 6.0], "a surrogate pair takes both advances");
    assert_eq!((t.font.family.as_str(), t.font.height, t.font.italic), ("Segoe UI", 24.0, true));
    assert_eq!((t.h_align, t.v_align, t.color, t.background), (HAlign::Right, VAlign::Baseline, [0, 255, 0, 255], None));
    // Upright, uniformly scaled.
    let [a, b, c, d, ..] = t.transform;
    assert!(a > 0.0 && d > 0.0 && b.abs() < 1e-9 && c.abs() < 1e-9 && (a - d).abs() < 1e-9, "{:?}", t.transform);
}

#[test]
fn emf_text_options_pdy_no_rect_ansi_and_glyph_indices() {
    // ETO_PDY: (dx, dy) pairs, of which dx is used. ETO_NO_RECT moves the dx offset to 56.
    let recs = [etext(true, 0, 0, &[0x61, 0x62], 0x2000 | 0x100, &[7, 1, 9, 2])];
    let p = parse(&emf(&recs, [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    assert_eq!(texts(&p)[0].dx, vec![7.0, 9.0]);
    // EXTTEXTOUTA decodes through the font's charset.
    let recs = [efont(-10, 0, 400, 0, 238, "F"), erec(37, &h32(1)), etext(false, 0, 0, &[0xB9], 0, &[])];
    let p = parse(&emf(&recs, [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    assert_eq!(texts(&p)[0].text, "ą");
    // Glyph indices name glyphs of a font we cannot match: left out.
    let recs = [etext(true, 0, 0, &[5, 6], 0x10, &[])];
    let p = parse(&emf(&recs, [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    assert!(texts(&p).is_empty());
}

#[test]
fn emf_smalltextout_reads_8_bit_and_16_bit_strings() {
    // x, y, count, options, mode, scales, (no bounds under ETO_NO_RECT), characters.
    let small = |opts: u32, chars: &[u8]| {
        let mut v = cat(vec![e32(3), e32(4), h32(2), h32(opts), h32(1), h32(0), h32(0)]);
        v.extend(chars);
        erec(108, &v)
    };
    let p = parse(&emf(&[small(0x100 | 0x200, b"hi")], [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    assert_eq!(texts(&p)[0].text, "hi");
    let p = parse(&emf(&[small(0x100, &[b'o', 0, b'k', 0])], [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    assert_eq!(texts(&p)[0].text, "ok");
}

/// An 8 × 8 one-bit DIB (palette white, black; stored bottom-up): `rows` top to bottom.
fn mono_dib(rows: [u8; 8]) -> Vec<u8> {
    let mut v = info_header(8, 8, 1, 32);
    v.extend([255, 255, 255, 0, 0, 0, 0, 0]);
    for r in rows.iter().rev() {
        v.extend([*r, 0, 0, 0]);
    }
    v
}

fn pattern_of(item: &Item) -> &crate::Tile {
    match item {
        Item::Pattern { tile, .. } => tile,
        other => panic!("expected a pattern fill: {other:?}"),
    }
}

/// Pixel (x, y) of a tile.
fn tile_px(t: &crate::Tile, x: usize, y: usize) -> [u8; 4] {
    let o = (y * t.width as usize + x) * 4;
    t.rgba[o..o + 4].try_into().unwrap()
}

#[test]
fn wmf_dib_pattern_brush_paints_its_rows_top_down() {
    let brush = wrec(0x0142, &cat(vec![h16(5), h16(0), mono_dib([0xFF, 0x80, 0, 0, 0, 0, 0, 0x01])]));
    let recs = [
        brush,
        // A pen after the brush: the brush took handle 0.
        wrec(0x02FA, &cat(vec![h16(0), h16(1), h16(0), h32(0x0000_00FF)])),
        wrec(0x012D, &h16(0)),
        wrec(0x012D, &h16(1)),
        wrec(0x041B, &cat(vec![h16(720), h16(1440), h16(0), h16(0)])),
    ];
    let p = parse(&wmf(&recs, Some([0, 0, 1440, 720]))).unwrap();
    assert_eq!(p.items.len(), 2, "the tile fill and the outline: {:?}", p.items);
    let t = pattern_of(&p.items[0]);
    assert_eq!((t.width, t.height), (8, 8));
    for x in 0..8 {
        assert_eq!(tile_px(t, x, 0), [0, 0, 0, 255], "top row is set: black");
    }
    assert_eq!(tile_px(t, 0, 1), [0, 0, 0, 255]);
    assert_eq!(tile_px(t, 1, 1), [255, 255, 255, 255]);
    assert_eq!(tile_px(t, 7, 7), [0, 0, 0, 255]);
    assert_eq!(tile_px(t, 0, 7), [255, 255, 255, 255]);
    // 96-dpi pixels of a picture in points, laid from the picture's corner.
    assert!(close(t.cell[0], 0.75) && close(t.cell[1], 0.75) && t.origin == [0.0, 0.0], "{:?} {:?}", t.cell, t.origin);
    assert_eq!(path_segs(&p.items[1]).2.map(|s| s.0), Some([255, 0, 0, 255]));
}

#[test]
fn hatch_brushes_draw_lines_over_the_background_mode() {
    let hatch = |style: i32| wrec(0x02FC, &cat(vec![h16(2), h32(0x00FF_0000), h16(style)]));
    let rect = wrec(0x041B, &cat(vec![h16(50), h16(50), h16(0), h16(0)]));
    // HS_CROSS over a transparent background.
    let p = wmf200(&[hatch(4), wrec(0x012D, &h16(0)), wrec(0x0102, &h16(1)), rect.clone()]);
    let t = pattern_of(&p.items[0]);
    let blue = [0, 0, 255, 255];
    assert_eq!((tile_px(t, 3, 7), tile_px(t, 7, 3)), (blue, blue));
    assert_eq!(tile_px(t, 3, 3)[3], 0, "gaps are transparent");
    // HS_BDIAGONAL over the opaque white background.
    let p = wmf200(&[hatch(3), wrec(0x012D, &h16(0)), rect.clone()]);
    let t = pattern_of(&p.items[0]);
    assert_eq!((tile_px(t, 0, 7), tile_px(t, 7, 0)), (blue, blue));
    assert_eq!(tile_px(t, 0, 0), [255, 255, 255, 255]);
    // An unknown hatch style draws no fill.
    let p = wmf200(&[hatch(9), wrec(0x012D, &h16(0)), rect]);
    assert!(p.items.iter().all(|i| !matches!(i, Item::Pattern { .. })), "{:?}", p.items);
}

#[test]
fn xor_mask_xor_draws_the_pattern_in_the_xor_colour() {
    // The see-through shadow: XOR a grey polygon, AND a checker pattern, XOR the polygon again.
    let square = wrec(0x0324, &cat(vec![h16(4), h16(0), h16(0), h16(40), h16(0), h16(40), h16(40), h16(0), h16(40)]));
    let recs = [
        wrec(0x0104, &h16(7)),
        wrec(0x02FC, &cat(vec![h16(0), h32(0x0080_8080), h16(0)])),
        wrec(0x012D, &h16(0)),
        square.clone(),
        wrec(0x0104, &h16(9)),
        wrec(0x0142, &cat(vec![h16(5), h16(0), mono_dib([0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA])])),
        wrec(0x012D, &h16(1)),
        wrec(0x041B, &cat(vec![h16(40), h16(40), h16(0), h16(0)])),
        wrec(0x0104, &h16(7)),
        wrec(0x012D, &h16(0)),
        square,
    ];
    let p = wmf200(&recs);
    assert_eq!(p.items.len(), 2, "only the AND pattern (and its outline) draw: {:?}", p.items);
    let t = pattern_of(&p.items[0]);
    // 0x55 = 0101 0101: white, black, …
    assert_eq!(tile_px(t, 0, 0)[3], 0, "white keeps what is below");
    assert_eq!(tile_px(t, 1, 0), [128, 128, 128, 255], "black takes the XOR colour");
}

#[test]
fn patblt_fills_with_the_brush() {
    let patblt = |rop: u32| wrec(0x061D, &cat(vec![h32(rop), h16(30), h16(40), h16(10), h16(20)]));
    let recs = [wrec(0x02FC, &cat(vec![h16(0), h32(0x00FF_0000), h16(0)])), wrec(0x012D, &h16(0)), patblt(0x00F0_0021), patblt(0x0055_0009)];
    let p = wmf200(&recs);
    assert_eq!(p.items.len(), 1, "DSTINVERT draws nothing: {:?}", p.items);
    let (x0, y0, x1, y1) = rect_extent(&p.items[0]);
    assert!(close(x0, 20.0) && close(y0, 10.0) && close(x1, 60.0) && close(y1, 40.0), "{x0} {y0} {x1} {y1}");
    assert_eq!(path_segs(&p.items[0]).1, Some([0, 0, 255, 255]));
}

#[test]
fn emf_dib_pattern_and_mono_brushes() {
    let dib = mono_dib([0xF0; 8]);
    let (info, bits) = dib.split_at(48);
    let brush = |typ: u32| {
        let f = [(8, 1u32), (12, 0), (16, 32), (20, info.len() as u32), (24, 32 + info.len() as u32), (28, bits.len() as u32)];
        erec_at(typ, 32 + dib.len(), &f, &[(32, info), (32 + info.len(), bits)])
    };
    let rect = erec(43, &cat(vec![e32(0), e32(0), e32(40), e32(40)]));
    let null_pen = erec(37, &h32(0x8000_0008));
    let p = parse(&emf(&[brush(94), erec(37, &h32(1)), null_pen.clone(), rect.clone()], [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    let t = pattern_of(&p.items[0]);
    assert_eq!((tile_px(t, 0, 0), tile_px(t, 7, 0)), ([0, 0, 0, 255], [255, 255, 255, 255]));
    // A mono brush paints in the text colour (red) and the background colour (green).
    let recs = [erec(24, &h32(0xFF)), erec(25, &h32(0xFF00)), brush(93), erec(37, &h32(1)), null_pen, rect];
    let p = parse(&emf(&recs, [0, 0, 200, 100], [0, 0, 5000, 2500])).unwrap();
    let t = pattern_of(&p.items[0]);
    assert_eq!((tile_px(t, 0, 0), tile_px(t, 7, 0)), ([255, 0, 0, 255], [0, 255, 0, 255]));
}

#[test]
fn hostile_text_and_brush_records_never_panic() {
    // A string longer than its record is cut to what the record holds.
    let mut v = cat(vec![h16(0), h16(0), h16(0x7FFF), h16(0)]);
    v.extend(b"abcd");
    let p = wmf200(&[wrec(0x0A32, &v)]);
    assert_eq!(texts(&p)[0].text, "abcd");
    // The rectangle flag without room for the rectangle draws nothing.
    let p = wmf200(&[wrec(0x0A32, &cat(vec![h16(0), h16(0), h16(1), h16(6)]))]);
    assert!(texts(&p).is_empty());
    // A face name with no terminator stops at 32 bytes.
    let p = wmf200(&[wfont(-10, 400, [0, 0, 0], 0, 0, &[b'A'; 40]), wrec(0x012D, &h16(0)), wtext(0, 0, b"x", 0, None, &[])]);
    assert_eq!(texts(&p)[0].font.family.len(), 32);
    // A pattern brush whose DIB is cut short, or too big for a tile, still takes its handle slot
    // and draws no fill.
    let short = wrec(0x0142, &cat(vec![h16(5), h16(0), info_header(8, 8, 1, 32)]));
    let mut huge = info_header(300, 300, 1, 0);
    huge.extend([255, 255, 255, 0, 0, 0, 0, 0]);
    huge.extend(vec![0u8; 40 * 300]);
    let huge = wrec(0x0142, &cat(vec![h16(5), h16(0), huge]));
    for b in [short, huge] {
        let recs = [
            b,
            wrec(0x02FA, &cat(vec![h16(0), h16(1), h16(0), h32(0xFF)])),
            wrec(0x012D, &h16(0)),
            wrec(0x012D, &h16(1)),
            wrec(0x041B, &cat(vec![h16(9), h16(9), h16(0), h16(0)])),
        ];
        let p = wmf200(&recs);
        assert_eq!(p.items.len(), 1, "{:?}", p.items);
        let (_, fill, stroke, _) = path_segs(&p.items[0]);
        assert_eq!((fill, stroke.map(|s| s.0)), (Some([255, 255, 255, 255]), Some([255, 0, 0, 255])));
    }
    // EMF: the string offset past the record, a huge count, a dx offset past the record.
    let bad = [
        erec_at(84, 80, &[(44, 2), (48, 4000), (52, 0)], &[]),
        erec_at(84, 84, &[(44, u32::MAX), (48, 76), (52, 0)], &[(76, &[b'a', 0, b'b', 0])]),
        erec_at(84, 80, &[(44, 2), (48, 76), (52, 0), (72, 0xFFFF_FFF0)], &[(76, &[b'a', 0, b'b', 0])]),
        erec_at(82, 20, &[(8, 1)], &[]),
        erec_at(94, 32, &[(8, 1), (16, 0xFFFF_FFFF), (20, 40), (24, 0), (28, 0xFFFF_FFFF)], &[]),
        erec_at(108, 40, &[(16, u32::MAX), (20, 0x300)], &[]),
    ];
    for r in bad {
        let _ = parse(&emf(&[r], [0, 0, 200, 100], [0, 0, 5000, 2500]));
    }
    let p = parse(&emf(
        &[erec_at(84, 84, &[(44, 2), (48, 76), (52, 0), (72, 0xFFFF_FFF0)], &[(76, &[b'a', 0, b'b', 0])])],
        [0, 0, 200, 100],
        [0, 0, 5000, 2500],
    ))
    .unwrap();
    assert!(texts(&p)[0].dx.is_empty(), "a dx array past the record is dropped");
    // Mutated text and brush files.
    let wfile = wmf(
        &[
            wfont(-20, 700, [1, 1, 1], 238, 450, b"Arial"),
            wrec(0x012D, &h16(0)),
            wrec(0x0142, &cat(vec![h16(5), h16(0), mono_dib([0x55; 8])])),
            wrec(0x012D, &h16(1)),
            wrec(0x012E, &h16(1)),
            wtext(5, 5, b"Hello", 6, Some([0, 0, 50, 50]), &[1, 2, 3, 4, 5]),
            wrec(0x041B, &cat(vec![h16(9), h16(9), h16(0), h16(0)])),
        ],
        Some([0, 0, 100, 100]),
    );
    let efile = emf(
        &[efont(24, 900, 400, 1, 0, "Segoe UI"), erec(37, &h32(1)), etext(true, 1, 2, &[0x41, 0xD83D, 0xDE00], 0x2000, &[1, 2, 3, 4, 5, 6])],
        [0, 0, 200, 100],
        [0, 0, 5000, 2500],
    );
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    for file in [&wfile, &efile] {
        for n in 0..=file.len() {
            let _ = parse(&file[..n]);
        }
        for _ in 0..1500 {
            let mut buf = file.clone();
            for _ in 0..6 {
                let at = rng.rnd() as usize % buf.len();
                buf[at] = rng.rnd() as u8;
            }
            let _ = parse(&buf);
        }
    }
}

#[test]
fn text_items_are_capped_like_any_other() {
    let t = wtext(0, 0, b"x", 0, None, &[]);
    let recs: Vec<Vec<u8>> = (0..MAX_ITEMS + 5).map(|_| t.clone()).collect();
    let p = wmf200(&recs);
    assert_eq!(p.items.len(), MAX_ITEMS);
}

#[test]
fn pattern_brush_pixels_count_against_the_byte_budget() {
    // 256 × 256 one-bit pattern brushes (256 KiB of RGBA each): far fewer than the object table
    // holds fit the budget, and the parse stops there.
    let mut dib = info_header(256, 256, 1, 0);
    dib.extend([255, 255, 255, 0, 0, 0, 0, 0]);
    dib.extend(vec![0x55u8; 32 * 256]);
    let brush = wrec(0x0142, &cat(vec![h16(5), h16(0), dib]));
    let mut recs: Vec<Vec<u8>> = (0..1100).map(|_| brush.clone()).collect();
    recs.push(wrec(0x041B, &cat(vec![h16(9), h16(9), h16(0), h16(0)])));
    let p = wmf200(&recs);
    assert!(p.items.is_empty(), "the parse stopped at the budget: {:?}", p.items.len());
}

fn item_region(item: &Item) -> Option<&[[f32; 4]]> {
    match item {
        Item::Path { region, .. } | Item::Bitmap { region, .. } | Item::Pattern { region, .. } => region.as_deref(),
        Item::Text(t) => t.region.as_deref(),
    }
}

#[test]
fn emf_intersect_clip_rect_limits_later_records_only() {
    let full = cat(vec![e32(0), e32(0), e32(80), e32(40)]);
    let clip = cat(vec![e32(10), e32(5), e32(40), e32(30)]);
    let p = parse(&emf(&[erec(43, &full), erec(30, &clip), erec(43, &full)], [0, 0, 80, 40], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 2);
    assert_eq!(item_region(&p.items[0]), None);
    assert_eq!(item_region(&p.items[1]), Some(&[[10.0, 5.0, 40.0, 30.0]][..]));
}

#[test]
fn emf_clip_follows_the_world_transform_and_save_restores_it() {
    let xf = cat([2.0f32, 0.0, 0.0, 2.0, 0.0, 0.0].iter().map(|v| v.to_le_bytes().to_vec()).collect());
    let rect = cat(vec![e32(0), e32(0), e32(10), e32(10)]);
    let clip = cat(vec![e32(0), e32(0), e32(5), e32(5)]);
    let p = parse(&emf(&[erec(35, &xf), erec(30, &clip), erec(43, &rect)], [0, 0, 20, 20], [0; 4])).unwrap();
    assert_eq!(item_region(&p.items[0]), Some(&[[0.0, 0.0, 10.0, 10.0]][..]));

    let full = cat(vec![e32(0), e32(0), e32(40), e32(20)]);
    let p =
        parse(&emf(&[erec(43, &full), erec(33, &[]), erec(30, &clip), erec(43, &full), erec(34, &e32(-1)), erec(43, &full)], [0, 0, 40, 20], [0; 4]))
            .unwrap();
    assert_eq!(p.items.len(), 3);
    assert_eq!(item_region(&p.items[0]), None);
    assert_eq!(item_region(&p.items[1]), Some(&[[0.0, 0.0, 5.0, 5.0]][..]));
    assert_eq!(item_region(&p.items[2]), None);
}

#[test]
fn emf_ext_select_clip_rgn_copies_and_intersects_rectangles() {
    let mut data = cat(vec![h32(32), h32(1), h32(2), h32(32), e32(0), e32(0), e32(50), e32(20)]);
    data.extend(cat(vec![e32(0), e32(0), e32(10), e32(20), e32(30), e32(0), e32(50), e32(20)]));
    let payload = cat(vec![h32(data.len() as u32), h32(5), data]);
    let rect = cat(vec![e32(0), e32(0), e32(50), e32(20)]);
    let p = parse(&emf(&[erec(75, &payload), erec(43, &rect)], [0, 0, 50, 20], [0; 4])).unwrap();
    assert_eq!(item_region(&p.items[0]), Some(&[[0.0, 0.0, 10.0, 20.0], [30.0, 0.0, 50.0, 20.0]][..]));

    let and = cat(vec![h32(0), h32(1)]);
    // A zero-length region selects the default clip, so the next rectangle is not clipped.
    let p = parse(&emf(&[erec(75, &payload), erec(75, &and), erec(43, &rect)], [0, 0, 50, 20], [0; 4])).unwrap();
    assert_eq!(item_region(&p.items[0]), None);

    let overlap = cat(vec![e32(40), e32(0), e32(80), e32(20)]);
    let p = parse(&emf(&[erec(75, &payload), erec(30, &overlap), erec(43, &rect)], [0, 0, 80, 20], [0; 4])).unwrap();
    assert_eq!(item_region(&p.items[0]), Some(&[[40.0, 0.0, 50.0, 20.0]][..]));
}

#[test]
fn emf_exclude_clip_rect_cuts_the_surface() {
    let hole = cat(vec![e32(20), e32(0), e32(40), e32(40)]);
    let rect = cat(vec![e32(0), e32(0), e32(80), e32(40)]);
    let p = parse(&emf(&[erec(29, &hole), erec(43, &rect)], [0, 0, 80, 40], [0; 4])).unwrap();
    assert_eq!(item_region(&p.items[0]), Some(&[[0.0, 0.0, 20.0, 40.0], [40.0, 0.0, 80.0, 40.0]][..]));
}

#[test]
fn an_empty_clip_drops_later_drawing() {
    let empty = cat(vec![e32(5), e32(5), e32(5), e32(8)]);
    let rect = cat(vec![e32(0), e32(0), e32(20), e32(10)]);
    let p = parse(&emf(&[erec(43, &rect), erec(30, &empty), erec(43, &rect)], [0, 0, 20, 10], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 1);
}

#[test]
fn wmf_intersect_clip_rect_limits_later_records() {
    let clip = cat(vec![h16(30), h16(40), h16(5), h16(10)]);
    let rect = cat(vec![h16(40), h16(80), h16(0), h16(0)]);
    let p = parse(&wmf(&[wrec(0x0416, &clip), wrec(0x041B, &rect)], None)).unwrap();
    assert_eq!(item_region(&p.items[0]), Some(&[[10.0, 5.0, 40.0, 30.0]][..]));
}

// ---- EMF+ ----

fn f32s(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// One EMF+ record: type, flags, size, data size, data (padded to 4).
fn prec(typ: u16, flags: u16, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    while !d.len().is_multiple_of(4) {
        d.push(0);
    }
    let mut v = typ.to_le_bytes().to_vec();
    v.extend(flags.to_le_bytes());
    v.extend(((12 + d.len()) as u32).to_le_bytes());
    v.extend((d.len() as u32).to_le_bytes());
    v.extend(d);
    v
}

/// An EMR_COMMENT carrying EMF+ records.
fn plus_comment(recs: &[Vec<u8>]) -> Vec<u8> {
    let body = recs.concat();
    let mut p = ((body.len() + 4) as u32).to_le_bytes().to_vec();
    p.extend(0x2B46_4D45u32.to_le_bytes());
    p.extend(body);
    erec(70, &p)
}

/// EMF+ header: dual (bit 0) or EMF+ only, 96 dpi.
fn plus_header(dual: bool) -> Vec<u8> {
    prec(0x4001, u16::from(dual), &cat(vec![h32(0xDBC0_1002), h32(0), h32(96), h32(96)]))
}

fn solid_pen(argb: u32, width: f32) -> Vec<u8> {
    cat(vec![h32(0xDBC0_1002), h32(0), h32(0), h32(0), f32s(&[width]), h32(0xDBC0_1002), h32(0), h32(argb)])
}

#[test]
fn emfplus_only_fill_rects_with_argb_colour() {
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_80FF), h32(1), f32s(&[10.0, 20.0, 30.0, 40.0])]));
    let p = parse(&emf(&[plus_comment(&[plus_header(false), fill])], [0; 4], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 1);
    let (segs, fill, stroke, _) = path_segs(&p.items[0]);
    assert_eq!(fill, Some([0x00, 0x80, 0xFF, 0xFF]));
    assert_eq!(stroke, None);
    // Geometry is placed relative to the drawing's bounding box (no frame given).
    assert_eq!((p.width, p.height), (30.0, 40.0));
    assert_eq!(segs[0], Seg::Move(0.0, 0.0));
    assert!(segs.contains(&Seg::Line(30.0, 40.0)));
}

#[test]
fn emfplus_dual_files_draw_only_the_emf_records() {
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFFFF_0000), h32(1), f32s(&[0.0, 0.0, 5.0, 5.0])]));
    let gdi = erec(43, &cat(vec![e32(0), e32(0), e32(9), e32(9)]));
    let p = parse(&emf(&[plus_comment(&[plus_header(true), fill]), gdi], [0; 4], [0; 4])).unwrap();
    assert_eq!(p.items.len(), 1);
    assert_eq!(path_segs(&p.items[0]).1, Some([255, 255, 255, 255]));
}

#[test]
fn emfplus_pen_object_draws_lines_under_a_scale_transform() {
    let pen = prec(0x4008, 0x0200 | 3, &solid_pen(0xFF00_FF00, 2.0));
    let scale = prec(0x402E, 0, &f32s(&[2.0, 3.0]));
    let lines = prec(0x400D, 3, &cat(vec![h32(2), f32s(&[1.0, 1.0, 4.0, 2.0])]));
    let p = parse(&emf(&[plus_comment(&[plus_header(false), pen, scale, lines])], [0; 4], [0; 4])).unwrap();
    let (segs, fill, stroke, _) = path_segs(&p.items[0]);
    // (1,1)-(4,2) scaled by (2,3) is (2,3)-(8,6), placed relative to its bounding box.
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(6.0, 3.0)]);
    assert_eq!(fill, None);
    let (c, w) = stroke.unwrap();
    assert_eq!(c, [0, 255, 0, 255]);
    assert!(w > 0.0);
}

#[test]
fn emfplus_path_object_fills_with_brush_object_and_restore_undoes_transform() {
    let brush = prec(0x4008, 0x0100 | 1, &cat(vec![h32(0xDBC0_1002), h32(0), h32(0x8012_3456)]));
    // Triangle: start, line, line|close.
    let path = prec(0x4008, 0x0300 | 2, &cat(vec![h32(0xDBC0_1002), h32(3), h32(0), f32s(&[0.0, 0.0, 10.0, 0.0, 0.0, 10.0]), vec![0, 1, 0x81]]));
    let save = prec(0x4025, 0, &h32(7));
    let mv = prec(0x402D, 0, &f32s(&[100.0, 0.0]));
    let restore = prec(0x4026, 0, &h32(7));
    let fill = prec(0x4014, 2, &h32(1));
    let p = parse(&emf(&[plus_comment(&[plus_header(false), brush, path, save, mv, restore, fill])], [0; 4], [0; 4])).unwrap();
    let (segs, fill, _, _) = path_segs(&p.items[0]);
    assert_eq!(fill, Some([0x12, 0x34, 0x56, 0x80]));
    assert_eq!(segs, &vec![Seg::Move(0.0, 0.0), Seg::Line(10.0, 0.0), Seg::Line(0.0, 10.0), Seg::Close]);
}

#[test]
fn emfplus_garbage_records_never_panic() {
    let mut s = 0x1234_5678u32;
    for _ in 0..300 {
        let mut recs = vec![plus_header(false)];
        for _ in 0..6 {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let typ = 0x4000 + (s >> 16) as u16 % 0x40;
            let data: Vec<u8> = (0..(s % 64)).map(|i| (s.rotate_left(i)) as u8).collect();
            recs.push(prec(typ, (s >> 8) as u16, &data));
        }
        let _ = parse(&emf(&[plus_comment(&recs)], [0; 4], [0; 4]));
    }
}

fn plus_pic(recs: Vec<Vec<u8>>) -> Picture {
    let mut all = vec![plus_header(false)];
    all.extend(recs);
    parse(&emf(&[plus_comment(&all)], [0; 4], [0; 4])).unwrap()
}

fn u16s(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A 2 x 2 32bpp ARGB bitmap image object: red, green / blue, white.
fn bitmap_2x2() -> Vec<u8> {
    let px = [0xFFFF_0000u32, 0xFF00_FF00, 0xFF00_00FF, 0xFFFF_FFFF];
    cat(vec![h32(0xDBC0_1002), h32(1), h32(2), h32(2), h32(8), h32(0x0026_200A), h32(0), px.iter().flat_map(|v| v.to_le_bytes()).collect()])
}

fn draw_image(id: u16, dst: [f32; 4]) -> Vec<u8> {
    prec(0x401A, id, &cat(vec![h32(0), h32(2), f32s(&[0.0, 0.0, 2.0, 2.0]), f32s(&dst)]))
}

#[test]
fn emfplus_draw_image_places_the_bitmap() {
    let p = plus_pic(vec![prec(0x4008, 0x0500 | 4, &bitmap_2x2()), draw_image(4, [10.0, 10.0, 20.0, 40.0])]);
    match &p.items[0] {
        Item::Bitmap { rect, width, height, rgba, .. } => {
            assert_eq!((*width, *height), (2, 2));
            assert_eq!(rect, &[0.0, 0.0, 20.0, 40.0]);
            assert_eq!(&rgba[..8], &[255, 0, 0, 255, 0, 255, 0, 255]);
            assert_eq!(&rgba[8..12], &[0, 0, 255, 255]);
        }
        other => panic!("expected a bitmap: {other:?}"),
    }
}

#[test]
fn emfplus_object_split_across_records_is_joined() {
    let obj = bitmap_2x2();
    let (a, b) = obj.split_at(20);
    let total = h32(obj.len() as u32);
    let first = prec(0x4008, 0x8000 | 0x0500 | 1, &cat(vec![total.clone(), a.to_vec()]));
    let second = prec(0x4008, 0x0500 | 1, b);
    let p = plus_pic(vec![first, second, draw_image(1, [0.0, 0.0, 4.0, 4.0])]);
    assert!(matches!(&p.items[0], Item::Bitmap { width: 2, height: 2, .. }));
}

#[test]
fn emfplus_compressed_png_image_decodes() {
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(3, 1, image::Rgba([1, 2, 3, 255]));
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let obj = cat(vec![h32(0xDBC0_1002), h32(1), h32(0), h32(0), h32(0), h32(0), h32(1), png]);
    let draw = prec(0x401A, 2, &cat(vec![h32(0), h32(2), f32s(&[0.0, 0.0, 3.0, 1.0]), f32s(&[0.0, 0.0, 3.0, 1.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0500 | 2, &obj), draw]);
    assert!(matches!(&p.items[0], Item::Bitmap { width: 3, height: 1, rgba, .. } if rgba[..4] == [1, 2, 3, 255]));
}

fn font_obj(family: &str, em: f32, unit: u32, style: u32) -> Vec<u8> {
    cat(vec![h32(0xDBC0_1002), f32s(&[em]), h32(unit), h32(style), h32(0), h32(family.encode_utf16().count() as u32), u16s(family)])
}

#[test]
fn emfplus_draw_string_with_font_and_centred_format() {
    let font = prec(0x4008, 0x0600 | 3, &font_obj("Calibri", 12.0, 2, 1));
    // String format: centre alignment, no clip.
    let fmt = prec(0x4008, 0x0700 | 4, &cat(vec![h32(0xDBC0_1002), h32(0x4000), h32(0), h32(1), h32(0)]));
    let s = prec(0x401C, 0x8000 | 3, &cat(vec![h32(0xFF11_2233), h32(4), h32(2), f32s(&[0.0, 0.0, 100.0, 20.0]), u16s("Hi")]));
    let p = plus_pic(vec![font, fmt, prec(0x400A, 0x8000, &cat(vec![h32(0xFFFF_FFFF), h32(1), f32s(&[0.0, 0.0, 100.0, 20.0])])), s]);
    let t = p.items.iter().find_map(|i| if let Item::Text(t) = i { Some(t) } else { None }).expect("a text item");
    assert_eq!(t.text, "Hi");
    assert_eq!(t.font.family, "Calibri");
    assert_eq!(t.font.weight, 700);
    assert!(close(t.font.height, -12.0));
    assert_eq!(t.color, [0x11, 0x22, 0x33, 0xFF]);
    assert_eq!(t.h_align, HAlign::Center);
    assert!(close(t.transform[4] as f32, 50.0));
    assert_eq!(t.background, None);
    assert_eq!(t.clip, None);
}

#[test]
fn emfplus_driver_string_uses_character_codes_and_positions() {
    let font = prec(0x4008, 0x0600, &font_obj("Arial", 10.0, 0, 0));
    let pos = f32s(&[5.0, 30.0, 11.0, 30.0]);
    let s = prec(0x4036, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), h32(0), h32(2), u16s("Ab"), pos]));
    let p = plus_pic(vec![prec(0x400A, 0x8000, &cat(vec![h32(0xFFFF_FFFF), h32(1), f32s(&[0.0, 0.0, 50.0, 50.0])])), font, s]);
    let Some(Item::Text(t)) = p.items.get(1) else { panic!("expected text: {:?}", p.items) };
    assert_eq!(t.text, "Ab");
    assert_eq!(t.dx, vec![6.0, 6.0]);
    assert_eq!(t.v_align, VAlign::Baseline);
    assert!(close(t.transform[4] as f32, 5.0) && close(t.transform[5] as f32, 30.0));
}

#[test]
fn emfplus_glyph_index_driver_strings_map_back_in_core_fonts_only() {
    // Glyph indices (no CmapLookup): 36, 69 are "Ab" in Arial's standard glyph order.
    let gi = || prec(0x4036, 0x8000, &cat(vec![h32(0xFF00_0000), h32(0), h32(0), h32(2), h16(36), h16(69), f32s(&[0.0, 0.0, 6.0, 0.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0600, &font_obj("Arial", 10.0, 0, 0)), gi()]);
    assert!(matches!(&p.items[0], Item::Text(t) if t.text == "Ab"), "{:?}", p.items);
    // An unknown font's glyph indices cannot be mapped: nothing is drawn.
    assert!(plus_pic(vec![prec(0x4008, 0x0600, &font_obj("Calibri", 10.0, 0, 0)), gi()]).items.is_empty());
}

#[test]
fn emfplus_linear_gradient_fills_with_a_shaded_tile() {
    // Red to blue across x = 0..100.
    let brush =
        cat(vec![h32(0xDBC0_1002), h32(4), h32(0), h32(0), f32s(&[0.0, 0.0, 100.0, 10.0]), h32(0xFFFF_0000), h32(0xFF00_00FF), h32(0), h32(0)]);
    let fill = prec(0x400A, 0, &cat(vec![h32(5), h32(1), f32s(&[0.0, 0.0, 100.0, 10.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0100 | 5, &brush), fill]);
    let Item::Pattern { tile, .. } = &p.items[0] else { panic!("expected a pattern: {:?}", p.items) };
    let px = |x: usize| &tile.rgba[x * 4..x * 4 + 4];
    assert!(px(0)[0] > 240 && px(0)[2] < 15, "left is red: {:?}", px(0));
    let right = (tile.width as usize - 2).min(127);
    assert!(px(right)[2] > 230 && px(right)[0] < 25, "right is blue: {:?}", px(right));
}

#[test]
fn emfplus_hatch_brush_fills_with_an_8x8_tile() {
    let brush = cat(vec![h32(0xDBC0_1002), h32(1), h32(4), h32(0xFF00_0000), h32(0xFFFF_FFFF)]);
    let fill = prec(0x400A, 0, &cat(vec![h32(0), h32(1), f32s(&[0.0, 0.0, 16.0, 16.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0100, &brush), fill]);
    assert!(matches!(&p.items[0], Item::Pattern { tile, .. } if tile.width == 8 && tile.height == 8));
}

#[test]
fn emfplus_clip_rect_cuts_fills_strokes_and_images() {
    let clip = prec(0x4032, 0, &f32s(&[0.0, 0.0, 50.0, 50.0]));
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[25.0, 25.0, 50.0, 50.0])]));
    let outside = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[60.0, 60.0, 5.0, 5.0])]));
    let pen = prec(0x4008, 0x0200 | 1, &solid_pen(0xFF00_FF00, 1.0));
    let line = prec(0x400D, 1, &cat(vec![h32(2), f32s(&[0.0, 10.0, 100.0, 10.0])]));
    let img = prec(0x4008, 0x0500 | 2, &bitmap_2x2());
    let p = plus_pic(vec![clip, fill, outside, pen, line, img, draw_image(2, [40.0, 0.0, 20.0, 20.0])]);
    assert_eq!(p.items.len(), 3, "{:?}", p.items);
    let (segs, ..) = path_segs(&p.items[0]);
    let xs: Vec<f32> = segs.iter().filter_map(|s| if let Seg::Move(x, _) | Seg::Line(x, _) = s { Some(*x) } else { None }).collect();
    // Picture coordinates start at the drawing's left edge (0): the square is cut at x = 50.
    assert!(xs.iter().all(|x| *x <= 50.0 + 1e-3), "{segs:?}");
    let (segs, ..) = path_segs(&p.items[1]);
    assert!(matches!(segs[1], Seg::Line(x, _) if close(x, 50.0)), "{segs:?}");
    let Item::Bitmap { rect, width, .. } = &p.items[2] else { panic!() };
    assert_eq!(*width, 1);
    assert!(close(rect[2], 10.0));
}

#[test]
fn emfplus_reset_clip_and_restore_bring_drawing_back() {
    let clip = prec(0x4032, 0, &f32s(&[0.0, 0.0, 1.0, 1.0]));
    let fill = || prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[10.0, 10.0, 5.0, 5.0])]));
    let p = plus_pic(vec![prec(0x4025, 0, &h32(1)), clip, fill(), prec(0x4026, 0, &h32(1)), fill()]);
    assert_eq!(p.items.len(), 1);
    let clip = prec(0x4032, 0, &f32s(&[0.0, 0.0, 1.0, 1.0]));
    let p = plus_pic(vec![clip, fill(), prec(0x4031, 0, &[]), fill()]);
    assert_eq!(p.items.len(), 1);
}

#[test]
fn emfplus_region_clip_intersects_rectangles() {
    // Region: intersect(rect 0,0,30,30 ; rect 10,10,100,100) = 10..30.
    let rect = |r: [f32; 4]| cat(vec![h32(0x1000_0000), f32s(&r)]);
    let region = cat(vec![h32(0xDBC0_1002), h32(2), h32(1), rect([0.0, 0.0, 30.0, 30.0]), rect([10.0, 10.0, 90.0, 90.0])]);
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[0.0, 0.0, 100.0, 100.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0400 | 6, &region), prec(0x4034, 6, &[]), fill]);
    assert_eq!((p.width, p.height), (20.0, 20.0));
}

#[test]
fn emfplus_region_path_cannot_read_into_its_sibling() {
    let rect = |r: [f32; 4]| cat(vec![h32(0x1000_0000), f32s(&r)]);
    // The path declares only its 12-byte header, but claims three points. The following
    // rectangle must not supply those points or be parsed twice as the next node.
    let path = cat(vec![h32(0x1000_0001), h32(12), h32(0), h32(3), h32(0)]);
    let region = cat(vec![h32(0xDBC0_1002), h32(2), h32(1), path, rect([10.0, 10.0, 20.0, 20.0])]);
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[0.0, 0.0, 100.0, 100.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0400 | 6, &region), prec(0x4034, 6, &[]), fill]);
    assert_eq!(p.items.len(), 1);
    assert_eq!((p.width, p.height), (100.0, 100.0));
}

#[test]
fn emfplus_region_path_keeps_its_shape() {
    let mut path_data = cat(vec![h32(0), h32(4), h32(0x4000)]);
    for (x, y) in [(10i16, 10i16), (30, 10), (30, 30), (10, 30)] {
        path_data.extend(x.to_le_bytes());
        path_data.extend(y.to_le_bytes());
    }
    path_data.extend([0, 1, 1, 0x81]);
    let path_node = cat(vec![h32(0x1000_0001), h32(path_data.len() as u32), path_data]);
    let region = cat(vec![h32(0xDBC0_1002), h32(1), path_node]);
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[0.0, 0.0, 100.0, 100.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0400 | 6, &region), prec(0x4034, 6, &[]), fill]);
    assert_eq!((p.width, p.height), (20.0, 20.0));
}

#[test]
fn emfplus_pie_and_arc() {
    // A quarter pie from 0 to 90 degrees in a 0..100 circle: centre, then (100, 50) round to (50, 100).
    let pie = prec(0x4010, 0x8000, &cat(vec![h32(0xFF00_0000), f32s(&[0.0, 90.0]), f32s(&[0.0, 0.0, 100.0, 100.0])]));
    let p = plus_pic(vec![pie]);
    let (segs, fill, ..) = path_segs(&p.items[0]);
    assert_eq!(fill, Some([0, 0, 0, 255]));
    assert_eq!(segs[0], Seg::Move(0.0, 0.0));
    assert_eq!(segs[1], Seg::Line(50.0, 0.0));
    assert!(matches!(segs[2], Seg::Cubic(.., x, y) if close(x, 0.0) && close(y, 50.0)), "{segs:?}");
    assert_eq!(segs[3], Seg::Close);
    // A half-circle arc stroke is two cubics.
    let pen = prec(0x4008, 0x0200, &solid_pen(0xFF00_0000, 1.0));
    let arc = prec(0x4012, 0, &cat(vec![f32s(&[180.0, -180.0]), f32s(&[0.0, 0.0, 100.0, 100.0])]));
    let p = plus_pic(vec![pen, arc]);
    let (segs, ..) = path_segs(&p.items[0]);
    assert_eq!(segs.len(), 3, "{segs:?}");
}

#[test]
fn emfplus_convex_clip_path_cuts_to_the_shape_not_its_box() {
    // A diamond clip path (0,50) (50,0) (100,50) (50,100): the corner of a full square fill is cut away.
    let diamond = cat(vec![h32(0xDBC0_1002), h32(4), h32(0), f32s(&[0.0, 50.0, 50.0, 0.0, 100.0, 50.0, 50.0, 100.0]), vec![0, 1, 1, 0x81]]);
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[0.0, 0.0, 100.0, 100.0])]));
    let p = plus_pic(vec![prec(0x4008, 0x0300 | 9, &diamond), prec(0x4033, 9, &[]), fill]);
    let (segs, ..) = path_segs(&p.items[0]);
    let pts: Vec<(f32, f32)> = segs.iter().filter_map(|s| if let Seg::Move(x, y) | Seg::Line(x, y) = s { Some((*x, *y)) } else { None }).collect();
    assert!(!pts.iter().any(|(x, y)| close(*x, 0.0) && close(*y, 0.0)), "corner kept: {pts:?}");
    assert!(pts.iter().all(|(x, y)| (x - 50.0).abs() + (y - 50.0).abs() <= 50.0 + 1e-3), "{pts:?}");
}

#[test]
fn emfplus_draw_string_wraps_to_the_layout_width() {
    let font = prec(0x4008, 0x0600, &font_obj("Arial", 10.0, 0, 0));
    let text = "alpha beta gamma delta";
    let s = prec(
        0x401C,
        0x8000,
        &cat(vec![h32(0xFF00_0000), h32(9), h32(text.encode_utf16().count() as u32), f32s(&[0.0, 0.0, 60.0, 100.0]), u16s(text)]),
    );
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFFFF_FFFF), h32(1), f32s(&[0.0, 0.0, 60.0, 100.0])]));
    let p = plus_pic(vec![fill, font, s]);
    let lines: Vec<&str> = p.items.iter().filter_map(|i| if let Item::Text(t) = i { Some(t.text.as_str()) } else { None }).collect();
    assert_eq!(lines, vec!["alpha beta", "gamma delta"]);
}

#[test]
fn emfplus_nested_metafile_image_draws_its_items_into_the_destination() {
    // The nested picture: a plain EMF with a 10 x 10 rectangle (white brush, black pen).
    let inner = emf(&[erec(43, &cat(vec![e32(0), e32(0), e32(10), e32(10)]))], [0; 4], [0; 4]);
    let obj = cat(vec![h32(0xDBC0_1002), h32(2), h32(3), h32(inner.len() as u32), inner]);
    let p = plus_pic(vec![prec(0x4008, 0x0500 | 3, &obj), draw_image(3, [0.0, 0.0, 40.0, 20.0])]);
    assert_eq!(p.items.len(), 1);
    assert_eq!((p.width, p.height), (40.0, 20.0));
    let (segs, fill, ..) = path_segs(&p.items[0]);
    assert_eq!(fill, Some([255, 255, 255, 255]));
    assert!(segs.contains(&Seg::Line(40.0, 20.0)), "{segs:?}");
}

/// Curves and a region fill, as an EMF+ only file (also written to `$DECKCRAFT_EMF_OUT` to look at).
fn curves_file() -> Vec<u8> {
    let pen = prec(0x4008, 0x0200 | 1, &solid_pen(0xFF20_60C0, 3.0));
    // A closed curve through a diamond's corners, filled orange.
    let diamond = f32s(&[100.0, 10.0, 190.0, 100.0, 100.0, 190.0, 10.0, 100.0]);
    let fill = prec(0x4016, 0x8000, &cat(vec![h32(0xFFE0_8020), f32s(&[0.5]), h32(4), diamond.clone()]));
    let outline = prec(0x4017, 1, &cat(vec![f32s(&[0.5]), h32(4), diamond]));
    // An open curve (a smoothed line chart): all four segments of five points.
    let line =
        prec(0x4018, 1, &cat(vec![f32s(&[0.5]), h32(0), h32(4), h32(5), f32s(&[210.0, 150.0, 260.0, 60.0, 310.0, 120.0, 360.0, 30.0, 410.0, 90.0])]));
    // A green rectangle region, filled.
    let region = cat(vec![h32(0xDBC0_1002), h32(0), h32(0x1000_0000), f32s(&[220.0, 160.0, 180.0, 30.0])]);
    let fill_region = prec(0x4013, 0x8000 | 2, &h32(0xFF30_A040));
    // A cream background first.
    let clear = prec(0x4009, 0, &h32(0xFFF4_EED8));
    let mut all = vec![plus_header(false), clear, pen, fill, outline, line, prec(0x4008, 0x0400 | 2, &region), fill_region];
    all.push(prec(0x4002, 0, &[]));
    emf(&[plus_comment(&all)], [0, 0, 420, 200], [0; 4])
}

#[test]
fn emfplus_cardinal_curves_and_region_fill() {
    let bytes = curves_file();
    if let Ok(path) = std::env::var("DECKCRAFT_EMF_OUT") {
        let _ = std::fs::write(path, &bytes);
    }
    let p = parse(&bytes).unwrap();
    assert_eq!(p.items.len(), 5, "{:?}", p.items.len());
    assert_eq!(path_segs(&p.items[0]).1, Some([0xF4, 0xEE, 0xD8, 0xFF]), "the clear comes first");
    // The closed curve: four cubics through the diamond's corners, closed.
    let (segs, fill, ..) = path_segs(&p.items[1]);
    assert_eq!(fill, Some([0xE0, 0x80, 0x20, 0xFF]));
    let cubics: Vec<(f32, f32)> = segs.iter().filter_map(|s| if let Seg::Cubic(.., x, y) = s { Some((*x, *y)) } else { None }).collect();
    assert_eq!(cubics.len(), 4, "{segs:?}");
    assert_eq!(segs.last(), Some(&Seg::Close));
    // The open curve: four cubics and no close; its controls bulge past the straight chords.
    let (segs, fill, stroke, _) = path_segs(&p.items[3]);
    assert_eq!((fill, stroke.map(|s| s.0)), (None, Some([0x20, 0x60, 0xC0, 0xFF])));
    assert_eq!(segs.iter().filter(|s| matches!(s, Seg::Cubic(..))).count(), 4);
    assert!(!segs.contains(&Seg::Close));
    // The region fill is its rectangle.
    let (segs, fill, ..) = path_segs(&p.items[4]);
    assert_eq!(fill, Some([0x30, 0xA0, 0x40, 0xFF]));
    assert_eq!(segs.iter().filter(|s| matches!(s, Seg::Line(..))).count(), 3, "{segs:?}");
}

#[test]
fn emfplus_curve_offsets_and_degenerate_input_are_safe() {
    let pen = prec(0x4008, 0x0200 | 1, &solid_pen(0xFF00_0000, 1.0));
    // Segments from point 2 of 5, asking for more than there are: two segments.
    let line = prec(0x4018, 1, &cat(vec![f32s(&[0.5]), h32(2), h32(99), h32(5), f32s(&[0.0, 0.0, 10.0, 5.0, 20.0, 0.0, 30.0, 5.0, 40.0, 0.0])]));
    let p = plus_pic(vec![pen.clone(), line]);
    let (segs, ..) = path_segs(&p.items[0]);
    assert_eq!(segs.iter().filter(|s| matches!(s, Seg::Cubic(..))).count(), 2, "{segs:?}");
    // One point, an offset past the end, a NaN tension, a missing region: nothing, no panic.
    let one = prec(0x4018, 1, &cat(vec![f32s(&[0.5]), h32(0), h32(1), h32(1), f32s(&[1.0, 1.0])]));
    let past = prec(0x4018, 1, &cat(vec![f32s(&[0.5]), h32(9), h32(9), h32(2), f32s(&[0.0, 0.0, 9.0, 9.0])]));
    let nan = prec(0x4017, 1, &cat(vec![f32s(&[f32::NAN]), h32(3), f32s(&[0.0, 0.0, 9.0, 9.0, 0.0, 9.0])]));
    let no_region = prec(0x4013, 0x8000 | 7, &h32(0xFF00_0000));
    let p = plus_pic(vec![pen, one, past, nan, no_region]);
    assert!(p.items.iter().all(|i| path_segs(i).0.iter().filter(|s| matches!(s, Seg::Cubic(..))).count() == 0));
}

#[test]
fn emfplus_clear_paints_the_whole_frame_and_transparent_clears_nothing() {
    let dot = prec(0x400A, 0x8000, &cat(vec![h32(0xFF00_0000), h32(1), f32s(&[40.0, 40.0, 10.0, 10.0])]));
    let clear = |argb: u32| prec(0x4009, 0, &h32(argb));
    // 100 × 50 device units (0..2500 × 0..1250 hundredths of a millimetre at 0.04 units each).
    let file = |recs: Vec<Vec<u8>>| {
        let mut all = vec![plus_header(false)];
        all.extend(recs);
        parse(&emf(&[plus_comment(&all)], [0, 0, 99, 49], [0, 0, 2500, 1250])).unwrap()
    };
    // A translated, clipped world doesn't move or cut the clear.
    let shift = prec(0x402D, 0, &f32s(&[30.0, 30.0]));
    let clip = prec(0x4032, 0, &f32s(&[0.0, 0.0, 5.0, 5.0]));
    let p = file(vec![shift, clip, clear(0xFFFF_FFFF), prec(0x4031, 0, &[]), dot.clone()]);
    assert_eq!(p.items.len(), 2);
    let (segs, fill, ..) = path_segs(&p.items[0]);
    assert_eq!(fill, Some([255, 255, 255, 255]));
    let xs: Vec<f32> = segs.iter().filter_map(|s| if let Seg::Move(x, _) | Seg::Line(x, _) = s { Some(*x) } else { None }).collect();
    let span = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
    assert!((span - p.width).abs() < 0.5, "clear spans {span} of {}", p.width);
    // Transparent: only the dot.
    assert_eq!(file(vec![clear(0x00FF_FFFF), dot.clone()]).items.len(), 1);
    // No frame: nothing to clear.
    assert_eq!(plus_pic(vec![clear(0xFFFF_FFFF), dot]).items.len(), 1);
}

#[test]
fn emfplus_wrap_line_linear_performance() {
    let font = prec(0x4008, 0x0600, &font_obj("Arial", 10.0, 0, 0));
    // Repeated words that wrap across lines
    let text = "word ".repeat(500);
    let s = prec(
        0x401C,
        0x8000,
        &cat(vec![h32(0xFF00_0000), h32(9), h32(text.encode_utf16().count() as u32), f32s(&[0.0, 0.0, 100.0, 1000.0]), u16s(&text)]),
    );
    let fill = prec(0x400A, 0x8000, &cat(vec![h32(0xFFFF_FFFF), h32(1), f32s(&[0.0, 0.0, 100.0, 1000.0])]));
    let p = plus_pic(vec![fill, font, s]);
    let text_items = p.items.iter().filter(|i| matches!(i, Item::Text(_))).count();
    assert!(text_items > 10, "wrapped into multiple lines: {text_items}");
}

#[test]
fn clip_polygon_work_bound() {
    let poly: Vec<(f64, f64)> = (0..2000).map(|i| (i as f64, i as f64)).collect();
    let clip: Vec<(f64, f64)> = (0..2000).map(|i| (i as f64, i as f64)).collect();
    let clipped = crate::cliprect::clip_polygon(&poly, &clip);
    assert!(clipped.is_empty(), "excessive polygon clip product rejected");
}
