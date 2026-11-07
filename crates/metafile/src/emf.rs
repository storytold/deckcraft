//! EMF reader (MS-EMF). Records are (type, size in bytes) followed by a payload; offsets inside a
//! record (bitmap info and bits) are relative to the record start, so each handler reads the record slice.

use std::sync::Arc;

use crate::bytes::Bytes;
use crate::canvas::{BS_HATCHED, BS_NULL, Brush, Canvas, LogFont, Obj, PS_NULL, PS_STYLE_MASK, Pen, Xform, colorref};
use crate::dib::{self, Blend};
use crate::geom::{self, ArcKind, P};
use crate::text::{ETO_GLYPH_INDEX, ETO_PDY, Font, MAX_CHARS, decode_bytes, decode_utf16};
use crate::{Error, Frame, MAX_PATH_OPS};

/// ExtTextOut options: no rectangle in the record (EMR_SMALLTEXTOUT, EmrText), and 8-bit
/// characters (EMR_SMALLTEXTOUT).
const ETO_NO_RECT: u32 = 0x0100;
const ETO_SMALL_CHARS: u32 = 0x0200;

const EMF_SIGNATURE: u32 = 0x464D_4520;
/// Hundredths of a millimetre to points.
const HMM_TO_PT: f64 = 72.0 / 2540.0;
/// SRCCOPY, the plain copy of a bitmap.
const SRCCOPY: u32 = 0x00CC_0020;
/// LogPenEx pen type bits; PS_COSMETIC pens are one device pixel wide.
const PS_TYPE_MASK: u32 = 0x000F_0000;
const PS_COSMETIC: u32 = 0;

pub(crate) fn is_emf(b: Bytes) -> bool {
    b.u32(0) == Some(1) && b.u32(40) == Some(EMF_SIGNATURE)
}

/// The picture frame: `rclFrame` (0.01 mm) mapped to device units through `szlDevice` and
/// `szlMillimeters`, sized in points. Without a usable frame, the device bounds stand in (1 unit = 1 point).
pub(crate) fn parse(input: Bytes) -> Result<(Canvas, Option<Frame>), Error> {
    if !is_emf(input) {
        return Err(Error::BadHeader);
    }
    let hsize = input.u32(4).ok_or(Error::BadHeader)? as usize;
    let bounds = rect32(input, 8).ok_or(Error::BadHeader)?;
    let [l, t, r, b] = bounds;
    let frame = frame_from_header(input).or_else(|| (r > l && b > t).then_some(Frame { rect: [l, t, r - l, b - t], size: None }));
    let mut cv = Canvas::new();
    if r > l && b > t {
        cv.set_clip_surface([l, t, r, b]);
    }
    if let Some(px_mm) = device_px_per_mm(input) {
        cv.set_device_mm(px_mm);
    }
    let mut plus = crate::emfplus::Plus::new();
    plus.frame = frame.as_ref().map(|f| f.rect);
    let mut parsed = 0usize;
    let mut pos = hsize.max(88);
    while !cv.full() {
        let (Some(typ), Some(size)) = (input.u32(pos), pos.checked_add(4).and_then(|o| input.u32(o))) else { break };
        let size = size as usize;
        if size < 8 || !size.is_multiple_of(4) {
            break;
        }
        let Some(rec) = input.slice(pos, size) else { break };
        if typ == 14 {
            break;
        }
        if typ == 70 && Bytes(rec).u32(12) == Some(crate::emfplus::EMF_PLUS) {
            if let Some(data) = Bytes(rec).tail(16) {
                plus.comment(&mut cv, data);
            }
        } else if plus.plays_gdi() {
            record(&mut cv, typ, Bytes(rec));
        }
        parsed += 1;
        let Some(next) = pos.checked_add(size) else { break };
        pos = next;
    }
    if parsed == 0 {
        return Err(Error::NoRecords);
    }
    Ok((cv, frame))
}

/// Device pixels per millimetre on each axis, from `szlDevice` (72) and `szlMillimeters` (80).
fn device_px_per_mm(input: Bytes) -> Option<(f64, f64)> {
    let (dw, dh) = (input.i32(72)?, input.i32(76)?);
    let (mw, mh) = (input.i32(80)?, input.i32(84)?);
    (dw > 0 && dh > 0 && mw > 0 && mh > 0).then(|| (f64::from(dw) / f64::from(mw), f64::from(dh) / f64::from(mh)))
}

/// The frame from `rclFrame` (offset 24), `szlDevice` (72) and `szlMillimeters` (80) in the header.
fn frame_from_header(input: Bytes) -> Option<Frame> {
    let [fl, ft, fr, fb] = rect32(input, 24)?;
    if fr <= fl || fb <= ft {
        return None;
    }
    // Device units per hundredth of a millimetre.
    let (px_mm_x, px_mm_y) = device_px_per_mm(input)?;
    let (sx, sy) = (px_mm_x / 100.0, px_mm_y / 100.0);
    let rect = [fl * sx, ft * sy, (fr - fl) * sx, (fb - ft) * sy];
    Some(Frame { rect, size: Some(((fr - fl) * HMM_TO_PT, (fb - ft) * HMM_TO_PT)) })
}

fn record(cv: &mut Canvas, typ: u32, r: Bytes) {
    if cv.full() {
        return;
    }
    let _ = match typ {
        9 => xy32(r, 8).map(|p| cv.set_window_ext(p)),
        10 => xy32(r, 8).map(|p| cv.set_window_org(p)),
        11 => xy32(r, 8).map(|p| cv.set_viewport_ext(p)),
        12 => xy32(r, 8).map(|p| cv.set_viewport_org(p)),
        17 => r.u32(8).map(|m| cv.set_map_mode(m)),
        19 => r.u32(8).map(|m| cv.set_polyfill(m)),
        20 => r.u32(8).map(|m| cv.set_rop2(m)),
        33 => {
            cv.save_dc();
            Some(())
        }
        34 => r.i32(8).map(|n| cv.restore_dc(n)),
        35 => xform(r, 8).map(|x| cv.modify_world(x, 4)),
        36 => xform(r, 8).zip(r.u32(32)).map(|(x, m)| cv.modify_world(x, m)),
        37 => r.u32(8).map(|i| cv.select(i)),
        38 => create_pen(cv, r),
        95 => create_ext_pen(cv, r),
        39 => create_brush(cv, r),
        93 | 94 => create_dib_brush(cv, r, typ == 93),
        82 => create_font(cv, r),
        18 => r.u32(8).map(|m| cv.set_bk_mode(m)),
        22 => r.u32(8).map(|a| cv.set_text_align(a)),
        24 => r.u32(8).map(|c| cv.set_text_color(c)),
        25 => r.u32(8).map(|c| cv.set_bk_color(c)),
        83 | 84 => ext_text_out(cv, r, typ == 84),
        108 => small_text_out(cv, r),
        40 => r.u32(8).map(|i| cv.delete_object(i)),
        29 => rect32(r, 8).map(|rect| cv.exclude_clip_rect(rect)),
        30 => rect32(r, 8).map(|rect| cv.intersect_clip_rect(rect)),
        75 => ext_select_clip(cv, r),
        27 => xy32(r, 8).map(|p| cv.move_to(p)),
        54 => xy32(r, 8).map(|p| cv.line_to(p)),
        42 => rect32(r, 8).map(|[l, t, rr, b]| cv.shape(&geom::ellipse(l, t, rr, b), true, true)),
        43 => rect32(r, 8).map(|[l, t, rr, b]| cv.shape(&geom::rect(l, t, rr, b), true, true)),
        44 => {
            let (cw, ch) = (r.i32(24), r.i32(28));
            let (Some(cw), Some(ch)) = (cw, ch) else { return };
            rect32(r, 8).map(|[l, t, rr, b]| {
                cv.shape(&geom::round_rect(l, t, rr, b, f64::from(cw), f64::from(ch)), true, true);
            })
        }
        45..=47 => arc32(r).map(|(bounds, s, e)| {
            let kind = match typ {
                45 => ArcKind::Arc,
                46 => ArcKind::Chord,
                _ => ArcKind::Pie,
            };
            let [l, t, rr, b] = bounds;
            cv.shape(&geom::arc_shape(l, t, rr, b, s, e, kind), kind != ArcKind::Arc, true);
        }),
        4 | 87 => counted(r, typ == 4).map(|pts| cv.poly(&pts, false, false, true)),
        3 | 86 => counted(r, typ == 3).map(|pts| cv.poly(&pts, true, true, true)),
        6 | 89 => counted(r, typ == 6).map(|pts| cv.line_chain(&pts, false, true)),
        // POLYBEZIER starts its own figure at the first point and leaves the current point alone;
        // POLYBEZIERTO continues from the current point.
        2 | 85 => counted(r, typ == 2).and_then(|pts| {
            let from = *pts.first()?;
            cv.bezier(from, pts.get(1..)?);
            Some(())
        }),
        5 | 88 => counted(r, typ == 5).map(|pts| cv.bezier_to(&pts)),
        7 | 90 => poly_groups(r, typ == 7).map(|(counts, pts)| cv.shape(&geom::subpaths(&pts, &counts, false), false, true)),
        8 | 91 => poly_groups(r, typ == 8).map(|(counts, pts)| cv.shape(&geom::subpaths(&pts, &counts, true), true, true)),
        59 => {
            cv.begin_path();
            Some(())
        }
        60 => {
            cv.end_path();
            Some(())
        }
        61 => {
            cv.close_figure();
            Some(())
        }
        62 => {
            cv.paint_path(true, false);
            Some(())
        }
        63 => {
            cv.paint_path(true, true);
            Some(())
        }
        64 => {
            cv.paint_path(false, true);
            Some(())
        }
        76 | 77 => bitblt(cv, r, typ == 77),
        80 => set_dibits(cv, r),
        81 => stretch_dibits(cv, r),
        114 => alphablend(cv, r),
        _ => Some(()),
    };
}

/// EMR_EXTSELECTCLIPRGN: `cbRgnData` bytes of `RGNDATA` (rectangles in logical units) and a region
/// mode. No region data selects the default clip.
fn ext_select_clip(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let cb = r.u32(8)? as usize;
    let mode = r.u32(12)?;
    if cb == 0 {
        cv.reset_clip();
        return Some(());
    }
    let header = 16;
    if cb < 32 {
        return Some(());
    }
    // RDH_RECTANGLES is the only region type GDI writes.
    if r.u32(header + 4)? != 1 {
        return Some(());
    }
    let n = (r.u32(header + 8)? as usize).min(crate::clip::MAX_CLIP_RECTS);
    let end = header.checked_add(cb)?;
    let mut rects = Vec::new();
    for i in 0..n {
        let o = header + 32 + i * 16;
        if o + 16 > end {
            break;
        }
        rects.push(rect32(r, o)?);
    }
    cv.select_clip_rects(&rects, mode);
    Some(())
}

/// Two little-endian i32 values at `off`, as a point.
fn xy32(r: Bytes, off: usize) -> Option<P> {
    Some((f64::from(r.i32(off)?), f64::from(r.i32(off + 4)?)))
}

/// RECTL (left, top, right, bottom) as i32 values widened to f64.
fn rect32(r: Bytes, off: usize) -> Option<[f64; 4]> {
    Some([f64::from(r.i32(off)?), f64::from(r.i32(off + 4)?), f64::from(r.i32(off + 8)?), f64::from(r.i32(off + 12)?)])
}

/// An i32 field as f64; 0 past the end.
fn i32s(r: Bytes, off: usize) -> f64 {
    r.i32(off).map_or(0.0, f64::from)
}

/// XFORM: six f32 values in M11, M12, M21, M22, Dx, Dy order.
fn xform(r: Bytes, off: usize) -> Option<Xform> {
    let mut x = [0.0f64; 6];
    for (i, v) in x.iter_mut().enumerate() {
        *v = f64::from(r.f32(off + i * 4)?);
    }
    Some(x)
}

/// ARC, CHORD, PIE: bounds at 8, start point at 24, end point at 32.
fn arc32(r: Bytes) -> Option<([f64; 4], P, P)> {
    Some((rect32(r, 8)?, xy32(r, 24)?, xy32(r, 32)?))
}

/// A point count from a record, rejected when it exceeds the path budget (before any allocation).
fn budget(n: u32) -> Option<usize> {
    let n = n as usize;
    (n <= MAX_PATH_OPS).then_some(n)
}

/// Point count at 24, then the points (32-bit or 16-bit) at 28.
fn counted(r: Bytes, wide: bool) -> Option<Vec<P>> {
    let n = budget(r.u32(24)?)?;
    r.tail(28)?.points(0, n, wide)
}

/// Subpath count at 24, total point count at 28, per-subpath counts at 32, then the points.
fn poly_groups(r: Bytes, wide: bool) -> Option<(Vec<usize>, Vec<P>)> {
    let n = budget(r.u32(24)?)?;
    // The record must hold the n counts before anything is allocated for them.
    let raw = r.slice(32, n.checked_mul(4)?)?;
    let counts: Vec<usize> = raw.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c) as usize).collect();
    let total = counts.iter().try_fold(0usize, |a, c| a.checked_add(*c))?;
    let total = budget(u32::try_from(total).ok()?)?;
    let pts = r.tail(n.checked_mul(4)?.checked_add(32)?)?.points(0, total, wide)?;
    Some((counts, pts))
}

/// EMR_CREATEPEN: handle, LogPen (style, width x/y, colour).
fn create_pen(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let slot = r.u32(8)? as usize;
    let style = r.u32(12)?;
    let width = r.i32(16)?;
    let color = r.u32(24)?;
    cv.add_object(Some(slot), Obj::Pen(Pen { style, width: f64::from(width), color: colorref(color) }));
    Some(())
}

/// EMR_EXTCREATEPEN: handle, bitmap offsets (unused), LogPenEx (style, width, brush style, colour).
/// A cosmetic pen's width is in device pixels (always 1), so it is a hairline; a BS_NULL brush draws
/// no line at all.
fn create_ext_pen(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let slot = r.u32(8)? as usize;
    let (style, width, brush, color) = (r.u32(28)?, r.u32(32)?, r.u32(36)?, r.u32(40)?);
    let width = if style & PS_TYPE_MASK == PS_COSMETIC { 0.0 } else { f64::from(width) };
    let style = if brush == BS_NULL { PS_NULL } else { style & PS_STYLE_MASK };
    cv.add_object(Some(slot), Obj::Pen(Pen { style, width, color: colorref(color) }));
    Some(())
}

/// EMR_CREATEBRUSHINDIRECT: handle, LogBrush32 (style, colour, hatch).
fn create_brush(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let slot = r.u32(8)? as usize;
    let style = r.u32(12)?;
    let color = r.u32(16)?;
    let mut b = Brush::new(style, colorref(color));
    if style == BS_HATCHED {
        b.hatch = r.u32(20).unwrap_or(0);
    }
    cv.add_object(Some(slot), Obj::Brush(b));
    Some(())
}

/// EMR_CREATEDIBPATTERNBRUSHPT (94) and EMR_CREATEMONOBRUSH (93): handle, colour usage, then the
/// bitmap info and bits as offset/size pairs. A mono brush (or one whose palette holds indices,
/// DIB_PAL_COLORS) takes the text and background colours. An undecodable bitmap leaves the slot
/// empty.
fn create_dib_brush(cv: &mut Canvas, r: Bytes, mono: bool) -> Option<()> {
    let slot = r.u32(8)? as usize;
    let usage = r.u32(12)?;
    let (bo, bl) = (r.u32(16)? as usize, r.u32(20)? as usize);
    let (po, pl) = (r.u32(24)? as usize, r.u32(28)? as usize);
    let img = dib::decode(Bytes(r.slice(bo, bl)?), Bytes(r.slice(po, pl)?), false)?;
    let b = Brush::pattern(img, mono || usage == 1)?;
    cv.add_object(Some(slot), Obj::Brush(b));
    Some(())
}

/// EMR_EXTCREATEFONTINDIRECTW: handle, then a LogFont (MS-EMF 2.2.13): height, width, escapement,
/// orientation, weight (i32 each), italic, underline, strike-out, charset and four more bytes, then
/// the face name in 32 UTF-16 units (NUL-terminated).
fn create_font(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let slot = r.u32(8)? as usize;
    let height = r.i32(12)?;
    let width = r.i32(16).unwrap_or(0);
    let escapement = r.i32(20).unwrap_or(0);
    let weight = r.i32(28).unwrap_or(0);
    let byte = |o: usize| r.0.get(o).copied().unwrap_or(0);
    let units: Vec<u16> = (0..32).map_while(|i| r.u16(40 + 2 * i)).take_while(|u| *u != 0).collect();
    let (family, _) = decode_utf16(&units, &[]);
    let font = Font {
        family: family.trim().to_string(),
        height: height as f32,
        width: width as f32,
        weight: weight.clamp(0, 1000) as u16,
        italic: byte(32) != 0,
        underline: byte(33) != 0,
        strike: byte(34) != 0,
        charset: byte(35),
    };
    cv.add_object(Some(slot), Obj::Font(Arc::new(LogFont { font, escapement: f64::from(escapement) })));
    Some(())
}

/// The dx array at `off` for `n` characters: i32 advances, or (dx, dy) pairs under ETO_PDY (only
/// dx is used). Empty when absent or short.
fn dx_array(r: Bytes, off: usize, n: usize, opts: u32) -> Vec<f32> {
    let step = if opts & ETO_PDY != 0 { 8 } else { 4 };
    if off == 0 {
        return Vec::new();
    }
    let dx: Vec<f32> = (0..n).map_while(|i| r.i32(off.checked_add(i.checked_mul(step)?)?)).map(|v| v as f32).collect();
    if dx.len() == n { dx } else { Vec::new() }
}

/// EMR_EXTTEXTOUTA (83) and EMR_EXTTEXTOUTW (84): bounds, graphics mode and scales, then an EmrText
/// at 36: reference point, character count, string offset, options, the rectangle (absent under
/// ETO_NO_RECT) and the dx offset. Offsets count from the record start. Glyph-index strings name
/// glyphs of a font the reader cannot match, so they are left out.
fn ext_text_out(cv: &mut Canvas, r: Bytes, wide: bool) -> Option<()> {
    let at = xy32(r, 36)?;
    let n = (r.u32(44)? as usize).min(MAX_CHARS);
    let off = r.u32(48)? as usize;
    let opts = r.u32(52)?;
    let (rect, dx_off) = if opts & ETO_NO_RECT != 0 { (None, r.u32(56)?) } else { (rect32(r, 56), r.u32(72)?) };
    if opts & ETO_GLYPH_INDEX != 0 {
        return None;
    }
    let dx = dx_array(r, dx_off as usize, n, opts);
    let (text, dx) = if wide {
        let raw = r.slice(off, n.checked_mul(2)?)?;
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        decode_utf16(&units, &dx)
    } else {
        decode_bytes(r.slice(off, n)?, cv.font_charset(), &dx)
    };
    cv.text(at, text, dx, opts, rect);
    Some(())
}

/// EMR_SMALLTEXTOUT (MS-EMF 2.3.5.37): x, y, character count, options, graphics mode, scales, the
/// bounds (absent under ETO_NO_RECT), then the characters: 8-bit under ETO_SMALL_CHARS, else UTF-16.
fn small_text_out(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let at = xy32(r, 8)?;
    let n = (r.u32(16)? as usize).min(MAX_CHARS);
    let opts = r.u32(20)?;
    if opts & ETO_GLYPH_INDEX != 0 {
        return None;
    }
    let (rect, off) = if opts & ETO_NO_RECT != 0 { (None, 36) } else { (rect32(r, 36), 52) };
    let (text, _) = if opts & ETO_SMALL_CHARS != 0 {
        decode_bytes(r.slice(off, n)?, cv.font_charset(), &[])
    } else {
        let raw = r.slice(off, n.checked_mul(2)?)?;
        let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        decode_utf16(&units, &[])
    };
    cv.text(at, text, Vec::new(), opts, rect);
    Some(())
}

/// BITBLT (76) and STRETCHBLT (77): dest at 24..40, rop at 40, source x/y at 44, bmi at 84, bits at
/// 92; STRETCHBLT adds the source w/h at 100. A source-less rop fills the destination instead.
fn bitblt(cv: &mut Canvas, r: Bytes, stretch: bool) -> Option<()> {
    let dst = [i32s(r, 24), i32s(r, 28), i32s(r, 32), i32s(r, 36)];
    let rop = r.u32(40)?;
    if Canvas::sourceless(rop) {
        cv.fill_rop(dst, rop);
        return Some(());
    }
    let (sw, sh) = if stretch { (i32s(r, 100), i32s(r, 104)) } else { (dst[2], dst[3]) };
    let src = [i32s(r, 44), i32s(r, 48), sw, sh];
    blit(cv, r, dst, src, (84, 92), Blend::Rop(rop))
}

/// ALPHABLEND: laid out like STRETCHBLT, with a BLENDFUNCTION at 40. Constant alpha at 42 applies to
/// every pixel; AlphaFormat at 43 with AC_SRC_ALPHA (1) also uses the per-pixel alpha.
fn alphablend(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let dst = [i32s(r, 24), i32s(r, 28), i32s(r, 32), i32s(r, 36)];
    let constant = *r.0.get(42)?;
    let per_pixel = (*r.0.get(43)? & 0x01) != 0;
    let src = [i32s(r, 44), i32s(r, 48), i32s(r, 100), i32s(r, 104)];
    blit(cv, r, dst, src, (84, 92), Blend::Alpha { constant, per_pixel })
}

/// STRETCHDIBITS: dest x/y at 24, src x/y/w/h at 32..48, bmi offset/size at 48, bits at 56, rop at 68,
/// dest w/h at 72.
fn stretch_dibits(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let dst = [i32s(r, 24), i32s(r, 28), i32s(r, 72), i32s(r, 76)];
    let src = [i32s(r, 32), i32s(r, 36), i32s(r, 40), i32s(r, 44)];
    let rop = r.u32(68)?;
    blit(cv, r, dst, src, (48, 56), Blend::Rop(rop))
}

/// SETDIBITSTODEVICE (MS-EMF 2.3.1.5): dest x/y at 24, src x/y at 32, size (cxSrc, cySrc) at 40, bmi
/// at 48, bits at 56, start scan at 68. The image keeps its own size; the bits begin at scan line
/// `iStartScan`, so the source y counts from there.
fn set_dibits(cv: &mut Canvas, r: Bytes) -> Option<()> {
    let (cx, cy) = (i32s(r, 40), i32s(r, 44));
    if cx <= 0.0 || cy <= 0.0 {
        return None;
    }
    let dst = [i32s(r, 24), i32s(r, 28), cx, cy];
    let src = [i32s(r, 32), i32s(r, 36) - f64::from(r.u32(68)?), cx, cy];
    blit(cv, r, dst, src, (48, 56), Blend::Rop(SRCCOPY))
}

/// Decodes the bitmap at `at` = (offset of the bmi offset/length pair, offset of the bits pair), both
/// inside the record, and stretches the source rectangle `src` into `dst`. Both are `[x, y, w, h]`; a
/// non-positive source size means the whole bitmap. `blend` decides how it combines.
fn blit(cv: &mut Canvas, r: Bytes, dst: [f64; 4], src: [f64; 4], at: (usize, usize), blend: Blend) -> Option<()> {
    let (bo, bl) = (r.u32(at.0)? as usize, r.u32(at.0 + 4)? as usize);
    let (po, pl) = (r.u32(at.1)? as usize, r.u32(at.1 + 4)? as usize);
    if pl == 0 {
        return None;
    }
    let info = Bytes(r.slice(bo, bl)?);
    let pixels = Bytes(r.slice(po, pl)?);
    let mut img = dib::decode(info, pixels, blend.per_pixel())?;
    dib::prepare(&mut img, blend);
    let (img, dst) = dib::clip_blit(&img, src.map(|v| v as i64), dst)?;
    cv.bitmap(dst, img);
    Some(())
}
