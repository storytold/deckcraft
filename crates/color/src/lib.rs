//! DeckCraft colour: sRGB colours, theme colour slots and DrawingML-style colour transforms
//! (tint, shade, luminance modulation/offset, alpha…), so theme-relative colours resolve the way
//! presentation files expect.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// An sRGB colour with straight alpha, components 0–255.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Rgba {
    fn default() -> Self {
        Rgba::BLACK
    }
}

impl Rgba {
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);
    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }
    /// `RRGGBB` or `#RRGGBB` (also `RRGGBBAA`).
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix('#').unwrap_or(s);
        if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match s.len() {
            6 => Some(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Rgba::rgba(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            3 => {
                let n = |i: usize| s.get(i..i + 1).and_then(|h| u8::from_str_radix(h, 16).ok()).map(|v| v * 17);
                Some(Rgba::rgb(n(0)?, n(1)?, n(2)?))
            }
            _ => None,
        }
    }
    /// `RRGGBB` uppercase (no alpha).
    pub fn hex(&self) -> String {
        format!("{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
    pub fn with_alpha(self, a: u8) -> Self {
        Rgba { a, ..self }
    }
    pub fn to_f32(self) -> [f32; 4] {
        [self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0, self.a as f32 / 255.0]
    }
    pub fn from_f64(r: f64, g: f64, b: f64, a: f64) -> Self {
        let c = |v: f64| if v.is_finite() { (v.clamp(0.0, 1.0) * 255.0).round() as u8 } else { 0 };
        Rgba { r: c(r), g: c(g), b: c(b), a: c(a) }
    }
    /// Relative luminance (WCAG), 0–1.
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let v = c as f64 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(self.r) + 0.7152 * lin(self.g) + 0.0722 * lin(self.b)
    }
    /// WCAG contrast ratio between two colours (1–21).
    pub fn contrast(self, other: Rgba) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
    pub fn to_hsl(self) -> (f64, f64, f64) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        if (max - min).abs() < 1e-12 {
            return (0.0, 0.0, l);
        }
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if (max - r).abs() < 1e-12 {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if (max - g).abs() < 1e-12 {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h * 60.0, s, l)
    }
    pub fn from_hsl(h: f64, s: f64, l: f64, a: u8) -> Self {
        let h = ((h % 360.0) + 360.0) % 360.0 / 360.0;
        let s = s.clamp(0.0, 1.0);
        let l = l.clamp(0.0, 1.0);
        if s == 0.0 {
            return Rgba::from_f64(l, l, l, a as f64 / 255.0);
        }
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let f = |t: f64| {
            let t = if t < 0.0 {
                t + 1.0
            } else if t > 1.0 {
                t - 1.0
            } else {
                t
            };
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        Rgba::from_f64(f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0), a as f64 / 255.0)
    }
    /// Linear interpolation (straight alpha).
    pub fn lerp(self, o: Rgba, t: f64) -> Rgba {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round().clamp(0.0, 255.0) as u8;
        Rgba { r: m(self.r, o.r), g: m(self.g, o.g), b: m(self.b, o.b), a: m(self.a, o.a) }
    }
}

/// The twelve theme colour slots, plus the mapped aliases (`bg1`, `tx1`…) and `phClr` (the
/// placeholder colour a style matrix entry is applied with).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SchemeSlot {
    Dk1,
    Lt1,
    Dk2,
    Lt2,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
    Hlink,
    FolHlink,
    Bg1,
    Tx1,
    Bg2,
    Tx2,
    PhClr,
}

impl SchemeSlot {
    pub const THEME: [SchemeSlot; 12] = [
        SchemeSlot::Dk1,
        SchemeSlot::Lt1,
        SchemeSlot::Dk2,
        SchemeSlot::Lt2,
        SchemeSlot::Accent1,
        SchemeSlot::Accent2,
        SchemeSlot::Accent3,
        SchemeSlot::Accent4,
        SchemeSlot::Accent5,
        SchemeSlot::Accent6,
        SchemeSlot::Hlink,
        SchemeSlot::FolHlink,
    ];
    pub fn xml_name(self) -> &'static str {
        match self {
            SchemeSlot::Dk1 => "dk1",
            SchemeSlot::Lt1 => "lt1",
            SchemeSlot::Dk2 => "dk2",
            SchemeSlot::Lt2 => "lt2",
            SchemeSlot::Accent1 => "accent1",
            SchemeSlot::Accent2 => "accent2",
            SchemeSlot::Accent3 => "accent3",
            SchemeSlot::Accent4 => "accent4",
            SchemeSlot::Accent5 => "accent5",
            SchemeSlot::Accent6 => "accent6",
            SchemeSlot::Hlink => "hlink",
            SchemeSlot::FolHlink => "folHlink",
            SchemeSlot::Bg1 => "bg1",
            SchemeSlot::Tx1 => "tx1",
            SchemeSlot::Bg2 => "bg2",
            SchemeSlot::Tx2 => "tx2",
            SchemeSlot::PhClr => "phClr",
        }
    }
    pub fn from_xml(s: &str) -> Option<Self> {
        Some(match s {
            "dk1" => SchemeSlot::Dk1,
            "lt1" => SchemeSlot::Lt1,
            "dk2" => SchemeSlot::Dk2,
            "lt2" => SchemeSlot::Lt2,
            "accent1" => SchemeSlot::Accent1,
            "accent2" => SchemeSlot::Accent2,
            "accent3" => SchemeSlot::Accent3,
            "accent4" => SchemeSlot::Accent4,
            "accent5" => SchemeSlot::Accent5,
            "accent6" => SchemeSlot::Accent6,
            "hlink" => SchemeSlot::Hlink,
            "folHlink" => SchemeSlot::FolHlink,
            "bg1" => SchemeSlot::Bg1,
            "tx1" => SchemeSlot::Tx1,
            "bg2" => SchemeSlot::Bg2,
            "tx2" => SchemeSlot::Tx2,
            "phClr" => SchemeSlot::PhClr,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            SchemeSlot::Dk1 | SchemeSlot::Tx1 => "Text/Background - Dark 1",
            SchemeSlot::Lt1 | SchemeSlot::Bg1 => "Text/Background - Light 1",
            SchemeSlot::Dk2 | SchemeSlot::Tx2 => "Text/Background - Dark 2",
            SchemeSlot::Lt2 | SchemeSlot::Bg2 => "Text/Background - Light 2",
            SchemeSlot::Accent1 => "Accent 1",
            SchemeSlot::Accent2 => "Accent 2",
            SchemeSlot::Accent3 => "Accent 3",
            SchemeSlot::Accent4 => "Accent 4",
            SchemeSlot::Accent5 => "Accent 5",
            SchemeSlot::Accent6 => "Accent 6",
            SchemeSlot::Hlink => "Hyperlink",
            SchemeSlot::FolHlink => "Followed Hyperlink",
            SchemeSlot::PhClr => "Placeholder colour",
        }
    }
}

/// A colour scheme: the twelve theme colours.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorScheme {
    pub name: String,
    pub colors: [Rgba; 12],
}

impl ColorScheme {
    pub fn get(&self, slot: SchemeSlot) -> Rgba {
        let i = match slot {
            SchemeSlot::Dk1 | SchemeSlot::Tx1 => 0,
            SchemeSlot::Lt1 | SchemeSlot::Bg1 => 1,
            SchemeSlot::Dk2 | SchemeSlot::Tx2 => 2,
            SchemeSlot::Lt2 | SchemeSlot::Bg2 => 3,
            SchemeSlot::Accent1 => 4,
            SchemeSlot::Accent2 => 5,
            SchemeSlot::Accent3 => 6,
            SchemeSlot::Accent4 => 7,
            SchemeSlot::Accent5 => 8,
            SchemeSlot::Accent6 => 9,
            SchemeSlot::Hlink => 10,
            SchemeSlot::FolHlink => 11,
            SchemeSlot::PhClr => 4,
        };
        self.colors.get(i).copied().unwrap_or(Rgba::BLACK)
    }
    pub fn set(&mut self, slot: SchemeSlot, c: Rgba) {
        if let Some(i) = SchemeSlot::THEME.iter().position(|s| *s == slot)
            && let Some(v) = self.colors.get_mut(i)
        {
            *v = c;
        }
    }
}

/// One colour transform, in file units (percentages ×1000, angles ×60000), applied in order.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "op", content = "val")]
pub enum ColorTransform {
    Tint(i32),
    Shade(i32),
    LumMod(i32),
    LumOff(i32),
    SatMod(i32),
    SatOff(i32),
    HueMod(i32),
    HueOff(i32),
    Alpha(i32),
    AlphaMod(i32),
    AlphaOff(i32),
    Comp,
    Inv,
    Gray,
    RedMod(i32),
    GreenMod(i32),
    BlueMod(i32),
}

impl ColorTransform {
    pub fn xml_name(&self) -> &'static str {
        match self {
            ColorTransform::Tint(_) => "tint",
            ColorTransform::Shade(_) => "shade",
            ColorTransform::LumMod(_) => "lumMod",
            ColorTransform::LumOff(_) => "lumOff",
            ColorTransform::SatMod(_) => "satMod",
            ColorTransform::SatOff(_) => "satOff",
            ColorTransform::HueMod(_) => "hueMod",
            ColorTransform::HueOff(_) => "hueOff",
            ColorTransform::Alpha(_) => "alpha",
            ColorTransform::AlphaMod(_) => "alphaMod",
            ColorTransform::AlphaOff(_) => "alphaOff",
            ColorTransform::Comp => "comp",
            ColorTransform::Inv => "inv",
            ColorTransform::Gray => "gray",
            ColorTransform::RedMod(_) => "redMod",
            ColorTransform::GreenMod(_) => "greenMod",
            ColorTransform::BlueMod(_) => "blueMod",
        }
    }
    pub fn value(&self) -> Option<i32> {
        match *self {
            ColorTransform::Tint(v)
            | ColorTransform::Shade(v)
            | ColorTransform::LumMod(v)
            | ColorTransform::LumOff(v)
            | ColorTransform::SatMod(v)
            | ColorTransform::SatOff(v)
            | ColorTransform::HueMod(v)
            | ColorTransform::HueOff(v)
            | ColorTransform::Alpha(v)
            | ColorTransform::AlphaMod(v)
            | ColorTransform::AlphaOff(v)
            | ColorTransform::RedMod(v)
            | ColorTransform::GreenMod(v)
            | ColorTransform::BlueMod(v) => Some(v),
            _ => None,
        }
    }
    pub fn from_xml(name: &str, val: Option<i32>) -> Option<Self> {
        let v = val.unwrap_or(0);
        Some(match name {
            "tint" => ColorTransform::Tint(v),
            "shade" => ColorTransform::Shade(v),
            "lumMod" => ColorTransform::LumMod(v),
            "lumOff" => ColorTransform::LumOff(v),
            "satMod" => ColorTransform::SatMod(v),
            "satOff" => ColorTransform::SatOff(v),
            "hueMod" => ColorTransform::HueMod(v),
            "hueOff" => ColorTransform::HueOff(v),
            "alpha" => ColorTransform::Alpha(v),
            "alphaMod" => ColorTransform::AlphaMod(v),
            "alphaOff" => ColorTransform::AlphaOff(v),
            "comp" => ColorTransform::Comp,
            "inv" => ColorTransform::Inv,
            "gray" => ColorTransform::Gray,
            "redMod" => ColorTransform::RedMod(v),
            "greenMod" => ColorTransform::GreenMod(v),
            "blueMod" => ColorTransform::BlueMod(v),
            _ => return None,
        })
    }
}

/// Apply transforms to a base colour.
pub fn apply(base: Rgba, transforms: &[ColorTransform]) -> Rgba {
    let mut c = base;
    for t in transforms {
        c = apply_one(c, *t);
    }
    c
}

fn pct(v: i32) -> f64 {
    v as f64 / 100_000.0
}

fn apply_one(c: Rgba, t: ColorTransform) -> Rgba {
    match t {
        // tint: towards white by (1 - value); shade: towards black by (1 - value). Office applies
        // these in linear light, which keeps mid-tones from washing out.
        ColorTransform::Tint(v) => {
            let k = pct(v).clamp(0.0, 1.0);
            map_linear(c, |x| x * k + (1.0 - k))
        }
        ColorTransform::Shade(v) => {
            let k = pct(v).clamp(0.0, 1.0);
            map_linear(c, |x| x * k)
        }
        ColorTransform::LumMod(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s, l * pct(v), c.a)
        }
        ColorTransform::LumOff(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s, l + pct(v), c.a)
        }
        ColorTransform::SatMod(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s * pct(v), l, c.a)
        }
        ColorTransform::SatOff(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h, s + pct(v), l, c.a)
        }
        ColorTransform::HueMod(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h * pct(v), s, l, c.a)
        }
        ColorTransform::HueOff(v) => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h + v as f64 / 60_000.0, s, l, c.a)
        }
        ColorTransform::Alpha(v) => c.with_alpha((pct(v).clamp(0.0, 1.0) * 255.0).round() as u8),
        ColorTransform::AlphaMod(v) => c.with_alpha((c.a as f64 * pct(v)).round().clamp(0.0, 255.0) as u8),
        ColorTransform::AlphaOff(v) => c.with_alpha((c.a as f64 + pct(v) * 255.0).round().clamp(0.0, 255.0) as u8),
        ColorTransform::Comp => {
            let (h, s, l) = c.to_hsl();
            Rgba::from_hsl(h + 180.0, s, l, c.a)
        }
        ColorTransform::Inv => Rgba { r: 255 - c.r, g: 255 - c.g, b: 255 - c.b, a: c.a },
        ColorTransform::Gray => {
            let y = (0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64).round().clamp(0.0, 255.0) as u8;
            Rgba { r: y, g: y, b: y, a: c.a }
        }
        ColorTransform::RedMod(v) => Rgba { r: (c.r as f64 * pct(v)).round().clamp(0.0, 255.0) as u8, ..c },
        ColorTransform::GreenMod(v) => Rgba { g: (c.g as f64 * pct(v)).round().clamp(0.0, 255.0) as u8, ..c },
        ColorTransform::BlueMod(v) => Rgba { b: (c.b as f64 * pct(v)).round().clamp(0.0, 255.0) as u8, ..c },
    }
}

fn to_lin(v: u8) -> f64 {
    let v = v as f64 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}
fn from_lin(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}
fn map_linear(c: Rgba, f: impl Fn(f64) -> f64) -> Rgba {
    Rgba::from_f64(from_lin(f(to_lin(c.r))), from_lin(f(to_lin(c.g))), from_lin(f(to_lin(c.b))), c.a as f64 / 255.0)
}

/// The colour-picker grid shown under each theme colour: lighter 80/60/40%, darker 25/50%
/// (for dark colours: lighter 90/75/50/25/10%).
pub fn theme_variants(c: Rgba) -> [(Vec<ColorTransform>, &'static str); 5] {
    let (_, _, l) = c.to_hsl();
    if l < 0.2 {
        [
            (vec![ColorTransform::LumMod(10000), ColorTransform::LumOff(90000)], "Lighter 90%"),
            (vec![ColorTransform::LumMod(25000), ColorTransform::LumOff(75000)], "Lighter 75%"),
            (vec![ColorTransform::LumMod(50000), ColorTransform::LumOff(50000)], "Lighter 50%"),
            (vec![ColorTransform::LumMod(75000), ColorTransform::LumOff(25000)], "Lighter 25%"),
            (vec![ColorTransform::LumMod(90000), ColorTransform::LumOff(10000)], "Lighter 10%"),
        ]
    } else if l > 0.8 {
        [
            (vec![ColorTransform::LumMod(95000)], "Darker 5%"),
            (vec![ColorTransform::LumMod(85000)], "Darker 15%"),
            (vec![ColorTransform::LumMod(75000)], "Darker 25%"),
            (vec![ColorTransform::LumMod(65000)], "Darker 35%"),
            (vec![ColorTransform::LumMod(50000)], "Darker 50%"),
        ]
    } else {
        [
            (vec![ColorTransform::LumMod(20000), ColorTransform::LumOff(80000)], "Lighter 80%"),
            (vec![ColorTransform::LumMod(40000), ColorTransform::LumOff(60000)], "Lighter 60%"),
            (vec![ColorTransform::LumMod(60000), ColorTransform::LumOff(40000)], "Lighter 40%"),
            (vec![ColorTransform::LumMod(75000)], "Darker 25%"),
            (vec![ColorTransform::LumMod(50000)], "Darker 50%"),
        ]
    }
}

/// Standard colours row of the colour picker.
pub const STANDARD_COLORS: [(Rgba, &str); 10] = [
    (Rgba::rgb(0xC0, 0x00, 0x00), "Dark Red"),
    (Rgba::rgb(0xFF, 0x00, 0x00), "Red"),
    (Rgba::rgb(0xFF, 0xC0, 0x00), "Orange"),
    (Rgba::rgb(0xFF, 0xFF, 0x00), "Yellow"),
    (Rgba::rgb(0x92, 0xD0, 0x50), "Light Green"),
    (Rgba::rgb(0x00, 0xB0, 0x50), "Green"),
    (Rgba::rgb(0x00, 0xB0, 0xF0), "Light Blue"),
    (Rgba::rgb(0x00, 0x70, 0xC0), "Blue"),
    (Rgba::rgb(0x00, 0x20, 0x60), "Dark Blue"),
    (Rgba::rgb(0x70, 0x30, 0xA0), "Purple"),
];

/// A few CSS-like preset colour names (`prstClr`).
pub fn preset(name: &str) -> Option<Rgba> {
    let c = match name {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "lime" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "gray" | "grey" => (128, 128, 128),
        "silver" => (192, 192, 192),
        "maroon" => (128, 0, 0),
        "navy" => (0, 0, 128),
        "olive" => (128, 128, 0),
        "purple" => (128, 0, 128),
        "teal" => (0, 128, 128),
        "orange" => (255, 165, 0),
        "pink" => (255, 192, 203),
        "brown" => (165, 42, 42),
        "gold" => (255, 215, 0),
        "ltGray" | "lightGray" => (211, 211, 211),
        "dkGray" | "darkGray" => (169, 169, 169),
        _ => return None,
    };
    Some(Rgba::rgb(c.0, c.1, c.2))
}

/// Every preset colour name a file may write (`ST_PresetColorVal`, with the later editions' aliases).
pub const PRESET_NAMES: [&str; 190] = [
    "aliceBlue",
    "antiqueWhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedAlmond",
    "blue",
    "blueViolet",
    "brown",
    "burlyWood",
    "cadetBlue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerBlue",
    "cornsilk",
    "crimson",
    "cyan",
    "darkBlue",
    "darkCyan",
    "darkGoldenrod",
    "darkGray",
    "darkGrey",
    "darkGreen",
    "darkKhaki",
    "darkMagenta",
    "darkOliveGreen",
    "darkOrange",
    "darkOrchid",
    "darkRed",
    "darkSalmon",
    "darkSeaGreen",
    "darkSlateBlue",
    "darkSlateGray",
    "darkSlateGrey",
    "darkTurquoise",
    "darkViolet",
    "dkBlue",
    "dkCyan",
    "dkGoldenrod",
    "dkGray",
    "dkGrey",
    "dkGreen",
    "dkKhaki",
    "dkMagenta",
    "dkOliveGreen",
    "dkOrange",
    "dkOrchid",
    "dkRed",
    "dkSalmon",
    "dkSeaGreen",
    "dkSlateBlue",
    "dkSlateGray",
    "dkSlateGrey",
    "dkTurquoise",
    "dkViolet",
    "deepPink",
    "deepSkyBlue",
    "dimGray",
    "dimGrey",
    "dodgerBlue",
    "firebrick",
    "floralWhite",
    "forestGreen",
    "fuchsia",
    "gainsboro",
    "ghostWhite",
    "gold",
    "goldenrod",
    "gray",
    "grey",
    "green",
    "greenYellow",
    "honeydew",
    "hotPink",
    "indianRed",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderBlush",
    "lawnGreen",
    "lemonChiffon",
    "lightBlue",
    "lightCoral",
    "lightCyan",
    "lightGoldenrodYellow",
    "lightGray",
    "lightGrey",
    "lightGreen",
    "lightPink",
    "lightSalmon",
    "lightSeaGreen",
    "lightSkyBlue",
    "lightSlateGray",
    "lightSlateGrey",
    "lightSteelBlue",
    "lightYellow",
    "ltBlue",
    "ltCoral",
    "ltCyan",
    "ltGoldenrodYellow",
    "ltGray",
    "ltGrey",
    "ltGreen",
    "ltPink",
    "ltSalmon",
    "ltSeaGreen",
    "ltSkyBlue",
    "ltSlateGray",
    "ltSlateGrey",
    "ltSteelBlue",
    "ltYellow",
    "lime",
    "limeGreen",
    "linen",
    "magenta",
    "maroon",
    "medAquamarine",
    "medBlue",
    "medOrchid",
    "medPurple",
    "medSeaGreen",
    "medSlateBlue",
    "medSpringGreen",
    "medTurquoise",
    "medVioletRed",
    "mediumAquamarine",
    "mediumBlue",
    "mediumOrchid",
    "mediumPurple",
    "mediumSeaGreen",
    "mediumSlateBlue",
    "mediumSpringGreen",
    "mediumTurquoise",
    "mediumVioletRed",
    "midnightBlue",
    "mintCream",
    "mistyRose",
    "moccasin",
    "navajoWhite",
    "navy",
    "oldLace",
    "olive",
    "oliveDrab",
    "orange",
    "orangeRed",
    "orchid",
    "paleGoldenrod",
    "paleGreen",
    "paleTurquoise",
    "paleVioletRed",
    "papayaWhip",
    "peachPuff",
    "peru",
    "pink",
    "plum",
    "powderBlue",
    "purple",
    "red",
    "rosyBrown",
    "royalBlue",
    "saddleBrown",
    "salmon",
    "sandyBrown",
    "seaGreen",
    "seaShell",
    "sienna",
    "silver",
    "skyBlue",
    "slateBlue",
    "slateGray",
    "slateGrey",
    "snow",
    "springGreen",
    "steelBlue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whiteSmoke",
    "yellow",
    "yellowGreen",
];

/// Every system colour name a file may write (`ST_SystemColorVal`).
pub const SYSTEM_NAMES: [&str; 30] = [
    "scrollBar",
    "background",
    "activeCaption",
    "inactiveCaption",
    "menu",
    "window",
    "windowFrame",
    "menuText",
    "windowText",
    "captionText",
    "activeBorder",
    "inactiveBorder",
    "appWorkspace",
    "highlight",
    "highlightText",
    "btnFace",
    "btnShadow",
    "grayText",
    "btnText",
    "inactiveCaptionText",
    "btnHighlight",
    "3dDkShadow",
    "3dLight",
    "infoText",
    "infoBk",
    "hotLight",
    "gradientActiveCaption",
    "gradientInactiveCaption",
    "menuHighlight",
    "menuBar",
];

/// Whether `name` is a preset colour name a file may write.
pub fn is_preset_name(name: &str) -> bool {
    PRESET_NAMES.contains(&name)
}

/// Whether `name` is a system colour name a file may write.
pub fn is_system_name(name: &str) -> bool {
    SYSTEM_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_hex_rejects_multiple_hashes() {
        assert_eq!(Rgba::from_hex("##FF0000"), None);
        assert_eq!(Rgba::from_hex("####FF0000"), None);
        assert_eq!(Rgba::from_hex("#FF0000"), Some(Rgba::rgb(255, 0, 0)));
        assert_eq!(Rgba::from_hex("FF0000"), Some(Rgba::rgb(255, 0, 0)));
        assert_eq!(Rgba::from_hex("#FF000080"), Some(Rgba::rgba(255, 0, 0, 128)));
    }

    #[test]
    fn from_hex_rejects_signs() {
        assert_eq!(Rgba::from_hex("+12345"), None);
        assert_eq!(Rgba::from_hex("#+1+2+3"), None);
        assert_eq!(Rgba::from_hex("+1+2+3+4"), None);
        assert_eq!(Rgba::from_hex("+12"), None);
    }

    #[test]
    fn hex_roundtrip() {
        assert_eq!(Rgba::from_hex("#156082"), Some(Rgba::rgb(0x15, 0x60, 0x82)));
        assert_eq!(Rgba::rgb(0x15, 0x60, 0x82).hex(), "156082");
        assert_eq!(Rgba::from_hex("fff"), Some(Rgba::WHITE));
        assert_eq!(Rgba::from_hex("zz0000"), None);
        assert_eq!(Rgba::from_hex("ééé"), None);
        assert_eq!(Rgba::from_hex(""), None);
    }

    #[test]
    fn hsl_roundtrip() {
        for c in [Rgba::rgb(10, 200, 30), Rgba::rgb(255, 0, 0), Rgba::rgb(128, 128, 128), Rgba::rgb(21, 96, 130)] {
            let (h, s, l) = c.to_hsl();
            let d = Rgba::from_hsl(h, s, l, 255);
            assert!(
                (d.r as i32 - c.r as i32).abs() <= 1 && (d.g as i32 - c.g as i32).abs() <= 1 && (d.b as i32 - c.b as i32).abs() <= 1,
                "{c:?} {d:?}"
            );
        }
    }

    #[test]
    fn transforms() {
        let c = Rgba::rgb(0x40, 0x80, 0xC0);
        assert_eq!(apply(c, &[ColorTransform::Alpha(50000)]).a, 128);
        let lighter = apply(c, &[ColorTransform::LumMod(20000), ColorTransform::LumOff(80000)]);
        assert!(lighter.luminance() > c.luminance());
        let darker = apply(c, &[ColorTransform::LumMod(50000)]);
        assert!(darker.luminance() < c.luminance());
        assert_eq!(apply(Rgba::WHITE, &[ColorTransform::Shade(0)]), Rgba::BLACK);
        assert_eq!(apply(Rgba::BLACK, &[ColorTransform::Tint(0)]), Rgba::WHITE);
        assert_eq!(apply(c, &[ColorTransform::Inv]), Rgba::rgb(0xBF, 0x7F, 0x3F));
        // hostile values never panic
        let _ = apply(c, &[ColorTransform::LumMod(i32::MAX), ColorTransform::HueOff(i32::MIN), ColorTransform::AlphaMod(i32::MIN)]);
    }

    #[test]
    fn contrast_ratio() {
        assert!((Rgba::BLACK.contrast(Rgba::WHITE) - 21.0).abs() < 0.01);
        assert!((Rgba::WHITE.contrast(Rgba::WHITE) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn scheme_slots() {
        let mut s = ColorScheme { name: "t".into(), colors: [Rgba::BLACK; 12] };
        s.set(SchemeSlot::Accent3, Rgba::WHITE);
        assert_eq!(s.get(SchemeSlot::Accent3), Rgba::WHITE);
        assert_eq!(s.get(SchemeSlot::Tx1), Rgba::BLACK);
        for slot in SchemeSlot::THEME {
            assert_eq!(SchemeSlot::from_xml(slot.xml_name()), Some(slot));
        }
    }
}
