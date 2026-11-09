//! Text records: the font (LOGFONT), the text string decoded from its character set, and the
//! neutral [`Text`] item. The metafile crate knows no fonts, so measuring, alignment against the
//! font's metrics and glyph drawing are left to the renderer.

/// A font chosen by a CREATEFONTINDIRECT record (MS-WMF 2.2.1.2 Font, MS-EMF 2.2.13 LogFont).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Font {
    /// The face name the file asks for (`"Arial"`, `"Wingdings"`); may be empty.
    pub family: String,
    /// LOGFONT height in text units: negative is the character (em) height, positive the cell
    /// height (ascent plus descent), 0 the default size.
    pub height: f32,
    /// Average character width in text units; 0 keeps the font's own proportions.
    pub width: f32,
    /// 100..=900 (400 regular, 700 bold); 0 is the default weight.
    pub weight: u16,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// LOGFONT character set (0 ANSI, 2 SYMBOL, 238 EASTEUROPE…).
    pub charset: u8,
}

/// SYMBOL_CHARSET: byte codes are the font's own symbol codes.
pub const SYMBOL_CHARSET: u8 = 2;

/// Where the reference point sits on the string horizontally (TA_LEFT, TA_CENTER, TA_RIGHT).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Where the reference point sits vertically (TA_TOP, TA_BASELINE, TA_BOTTOM): the top of the
/// character cell, the baseline or the bottom of the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    #[default]
    Top,
    Baseline,
    Bottom,
}

/// One string drawn by EXTTEXTOUT, TEXTOUT or SMALLTEXTOUT.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// Text space to picture space as affine coefficients `[a, b, c, d, e, f]` (`x' = a·x + c·y + e`,
    /// `y' = b·x + d·y + f`, kurbo's order). Text space has x along the baseline and y down, in the
    /// metafile's logical units, with the reference point at the origin. Escapement and the
    /// mapping are folded in; glyphs are never mirrored.
    pub transform: [f64; 6],
    pub text: String,
    /// Advance after each character of `text`, in text units (the dx array). Empty when the file
    /// gives none: the font's own advances apply.
    pub dx: Vec<f32>,
    pub font: Font,
    pub color: [u8; 4],
    /// Colour filled behind the character cells (OPAQUE background mode); None when transparent.
    pub background: Option<[u8; 4]>,
    pub h_align: HAlign,
    pub v_align: VAlign,
    /// Clip rectangle `[x0, y0, x1, y1]` in picture units (ETO_CLIPPED).
    pub clip: Option<[f32; 4]>,
    /// Clip region from the device context, in picture units. None is the default clip. Drawn
    /// together with [`Text::clip`] when both are set.
    pub region: Option<Vec<[f32; 4]>>,
}

/// TA_UPDATECP: the string starts at the current position, which moves past it.
pub(crate) const TA_UPDATECP: u32 = 0x0001;
/// ETO_OPAQUE fills the rectangle with the background colour; ETO_CLIPPED clips the text to it.
pub(crate) const ETO_OPAQUE: u32 = 0x0002;
pub(crate) const ETO_CLIPPED: u32 = 0x0004;
/// The string holds glyph indices, not characters (drawn only when the font matches).
pub(crate) const ETO_GLYPH_INDEX: u32 = 0x0010;
/// The dx array holds (dx, dy) pairs.
pub(crate) const ETO_PDY: u32 = 0x2000;
/// Longest string kept from one record.
pub(crate) const MAX_CHARS: usize = 16_384;

/// Horizontal and vertical alignment from a TA_* word (the vertical bits are 0 top, 8 bottom,
/// 24 baseline; the horizontal ones 0 left, 2 right, 6 center).
pub(crate) fn align(ta: u32) -> (HAlign, VAlign) {
    let h = match ta & 0x06 {
        6 => HAlign::Center,
        2 => HAlign::Right,
        _ => HAlign::Left,
    };
    let v = match ta & 0x18 {
        0x18 => VAlign::Baseline,
        0x08 => VAlign::Bottom,
        _ => VAlign::Top,
    };
    (h, v)
}

/// Decodes 8-bit text in `charset` to one character per byte, and merges `dx` to match: a
/// double-byte character set's lead and trail bytes become one blank (its glyphs are not
/// decoded) whose advance is the sum of both. `dx` may be empty.
pub(crate) fn decode_bytes(bytes: &[u8], charset: u8, dx: &[f32]) -> (String, Vec<f32>) {
    let mut s = String::with_capacity(bytes.len());
    let mut out_dx = Vec::with_capacity(dx.len().min(bytes.len()));
    let dbcs = matches!(charset, 128 | 129 | 130 | 134 | 136);
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let n = if dbcs && b >= 0x81 && i + 1 < bytes.len() { 2 } else { 1 };
        let c = if n == 2 { ' ' } else { byte_char(b, charset) };
        s.push(c);
        if !dx.is_empty() {
            out_dx.push((i..i + n).filter_map(|k| dx.get(k)).sum());
        }
        i += n;
    }
    (s, out_dx)
}

/// UTF-16 text to a string with one dx entry per character: a surrogate pair takes the sum of
/// its two advances. Unpaired surrogates become U+FFFD.
pub(crate) fn decode_utf16(units: &[u16], dx: &[f32]) -> (String, Vec<f32>) {
    let mut s = String::with_capacity(units.len());
    let mut out_dx = Vec::with_capacity(dx.len().min(units.len()));
    let mut i = 0;
    for r in char::decode_utf16(units.iter().copied()) {
        let c = r.unwrap_or(char::REPLACEMENT_CHARACTER);
        let n = c.len_utf16();
        s.push(c);
        if !dx.is_empty() {
            out_dx.push((i..i + n).filter_map(|k| dx.get(k)).sum());
        }
        i += n;
    }
    (s, out_dx)
}

/// One byte in a single-byte Windows code page. Symbol fonts keep their codes in the private-use
/// range (U+F000 + code), where symbol fonts map them.
fn byte_char(b: u8, charset: u8) -> char {
    if b < 0x80 && charset != SYMBOL_CHARSET {
        return char::from(b);
    }
    let cp = match charset {
        SYMBOL_CHARSET => 0xF000 | u32::from(b),
        238 => high(b, &CP1250),
        204 => high(b, &CP1251),
        _ => high(b, &CP1252),
    };
    char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER)
}

/// The code point of byte `b` (0x80..=0xFF) from a table of the upper half; 0 entries are
/// undefined bytes, drawn as U+FFFD.
fn high(b: u8, table: &[u16; 128]) -> u32 {
    match table.get(usize::from(b.wrapping_sub(0x80))) {
        Some(0) | None => 0xFFFD,
        Some(v) => u32::from(*v),
    }
}

/// Windows-1252 (Western) 0x80..=0xFF: the upper half of Latin-1 with typographic marks in 0x80..=0x9F.
const CP1252: [u16; 128] = {
    let mut t = [0u16; 128];
    let low: [u16; 32] = [
        0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C,
        0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
    ];
    let mut i = 0;
    while i < 128 {
        t[i] = if i < 32 { low[i] } else { 0x80 + i as u16 };
        i += 1;
    }
    t
};

/// Windows-1250 (Central European) 0x80..=0xFF.
const CP1250: [u16; 128] = [
    0x20AC, 0, 0x201A, 0, 0x201E, 0x2026, 0x2020, 0x2021, 0, 0x2030, 0x0160, 0x2039, 0x015A, 0x0164, 0x017D, 0x0179, //
    0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0, 0x2122, 0x0161, 0x203A, 0x015B, 0x0165, 0x017E, 0x017A, //
    0x00A0, 0x02C7, 0x02D8, 0x0141, 0x00A4, 0x0104, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x015E, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x017B, //
    0x00B0, 0x00B1, 0x02DB, 0x0142, 0x00B4, 0x00B5, 0x00B6, 0x00B7, 0x00B8, 0x0105, 0x015F, 0x00BB, 0x013D, 0x02DD, 0x013E, 0x017C, //
    0x0154, 0x00C1, 0x00C2, 0x0102, 0x00C4, 0x0139, 0x0106, 0x00C7, 0x010C, 0x00C9, 0x0118, 0x00CB, 0x011A, 0x00CD, 0x00CE, 0x010E, //
    0x0110, 0x0143, 0x0147, 0x00D3, 0x00D4, 0x0150, 0x00D6, 0x00D7, 0x0158, 0x016E, 0x00DA, 0x0170, 0x00DC, 0x00DD, 0x0162, 0x00DF, //
    0x0155, 0x00E1, 0x00E2, 0x0103, 0x00E4, 0x013A, 0x0107, 0x00E7, 0x010D, 0x00E9, 0x0119, 0x00EB, 0x011B, 0x00ED, 0x00EE, 0x010F, //
    0x0111, 0x0144, 0x0148, 0x00F3, 0x00F4, 0x0151, 0x00F6, 0x00F7, 0x0159, 0x016F, 0x00FA, 0x0171, 0x00FC, 0x00FD, 0x0163, 0x02D9,
];

/// Windows-1251 (Cyrillic) 0x80..=0xFF; 0xC0..=0xFF are А..я in order.
const CP1251: [u16; 128] = {
    let low: [u16; 64] = [
        0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409, 0x2039, 0x040A, 0x040C, 0x040B, 0x040F, //
        0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F, //
        0x00A0, 0x040E, 0x045E, 0x0408, 0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407, //
        0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB, 0x0458, 0x0405, 0x0455, 0x0457,
    ];
    let mut t = [0u16; 128];
    let mut i = 0;
    while i < 128 {
        t[i] = if i < 64 { low[i] } else { 0x0410 + (i as u16 - 64) };
        i += 1;
    }
    t
};
