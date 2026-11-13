//! Attribute values: lengths, colours, number lists and transform lists.

use deckcraft_color::Rgba;
use kurbo::Affine;

/// A length in user units. `px` and a bare number are the same unit (96 to the inch); percentages
/// need a viewport this reader does not track, and return `None`.
pub fn length(s: &str) -> Option<f64> {
    length_em(s, 16.0)
}

/// [`length`] with the font size an `em` or `ex` is measured against.
pub fn length_em(s: &str, em: f64) -> Option<f64> {
    let s = s.trim();
    let end = s.char_indices().rev().take_while(|(_, c)| c.is_ascii_alphabetic() || *c == '%').last().map(|(i, _)| i).unwrap_or(s.len());
    let (num, unit) = s.split_at(end);
    let v: f64 = num.trim().parse().ok()?;
    if !v.is_finite() {
        return None;
    }
    let k = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "px" => 1.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        "em" => em,
        "ex" => em / 2.0,
        _ => return None,
    };
    Some(v * k).filter(|v| v.is_finite())
}

/// Comma- or whitespace-separated numbers (`points`, `viewBox`, `dasharray`, transform arguments).
pub fn numbers(s: &str) -> Vec<f64> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .take(4096)
        .collect()
}

/// A paint value: `none`/`transparent`, `#rgb[a]`/`#rrggbb[aa]`, `rgb()`/`rgba()` or a colour name.
/// `url(#…)` (a gradient) is resolved by the caller, which has the document.
pub fn color(s: &str) -> Option<Rgba> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 4 {
            let n = |i: usize| hex.get(i..i + 1).and_then(|h| u8::from_str_radix(h, 16).ok()).map(|v| v * 17);
            return Some(Rgba::rgba(n(0)?, n(1)?, n(2)?, n(3)?));
        }
        return Rgba::from_hex(hex);
    }
    let lower = s.to_ascii_lowercase();
    let (head, rest) = lower.split_once('(')?;
    let body = rest.strip_suffix(')')?;
    if head != "rgb" && head != "rgba" {
        return None;
    }
    let parts: Vec<&str> = body.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).collect();
    let ch = |i: usize| -> Option<u8> {
        let t = parts.get(i)?;
        let v = match t.strip_suffix('%') {
            Some(p) => p.parse::<f64>().ok()? * 2.55,
            None => t.parse::<f64>().ok()?,
        };
        Some(v.round().clamp(0.0, 255.0) as u8)
    };
    let a = match (parts.get(3), head) {
        (Some(t), _) => match t.strip_suffix('%') {
            Some(p) => (p.parse::<f64>().ok()? / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8,
            None => (t.parse::<f64>().ok()? * 255.0).round().clamp(0.0, 255.0) as u8,
        },
        (None, _) => 255,
    };
    Some(Rgba::rgba(ch(0)?, ch(1)?, ch(2)?, a))
}

/// The CSS colour names pictures use. Anything else is not a colour this reader knows.
pub fn named(name: &str) -> Option<Rgba> {
    let n = name.trim().to_ascii_lowercase();
    let rgb = match n.as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "lime" | "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" | "aqua" => (0, 255, 255),
        "magenta" | "fuchsia" => (255, 0, 255),
        "silver" => (192, 192, 192),
        "gray" | "grey" => (128, 128, 128),
        "maroon" => (128, 0, 0),
        "olive" => (128, 128, 0),
        "purple" => (128, 0, 128),
        "teal" => (0, 128, 128),
        "navy" => (0, 0, 128),
        "orange" => (255, 165, 0),
        "brown" => (165, 42, 42),
        "pink" => (255, 192, 203),
        "gold" => (255, 215, 0),
        "transparent" => return Some(Rgba::TRANSPARENT),
        _ => return None,
    };
    Some(Rgba::rgb(rgb.0, rgb.1, rgb.2))
}

/// A `transform` list, applied left to right as SVG does. Operations may be separated by commas.
pub fn transform(s: &str) -> Affine {
    let mut m = Affine::IDENTITY;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest.get(..open).unwrap_or("").trim().trim_start_matches(',').trim().to_ascii_lowercase();
        let Some(close) = rest.get(open..).and_then(|r| r.find(')')) else { break };
        let args = numbers(rest.get(open + 1..open + close).unwrap_or(""));
        let a = args.as_slice();
        let t = match name.as_str() {
            "translate" => Affine::translate((a.first().copied().unwrap_or(0.0), a.get(1).copied().unwrap_or(0.0))),
            "scale" => {
                let sx = a.first().copied().unwrap_or(1.0);
                Affine::scale_non_uniform(sx, a.get(1).copied().unwrap_or(sx))
            }
            "rotate" => {
                let r = Affine::rotate(a.first().copied().unwrap_or(0.0).to_radians());
                match (a.get(1), a.get(2)) {
                    (Some(cx), Some(cy)) => Affine::translate((*cx, *cy)) * r * Affine::translate((-*cx, -*cy)),
                    _ => r,
                }
            }
            "matrix" if a.len() >= 6 => Affine::new([a[0], a[1], a[2], a[3], a[4], a[5]]),
            // `Affine::skew` takes tangent factors, not radians.
            "skewx" => Affine::skew(a.first().copied().unwrap_or(0.0).to_radians().tan(), 0.0),
            "skewy" => Affine::skew(0.0, a.first().copied().unwrap_or(0.0).to_radians().tan()),
            _ => Affine::IDENTITY,
        };
        m *= t;
        rest = rest.get(open + close + 1..).unwrap_or("");
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_accept_the_units_a_picture_uses() {
        assert_eq!(length("12"), Some(12.0));
        assert_eq!(length("12px"), Some(12.0));
        assert_eq!(length("10pt").map(|v| (v * 100.0).round()), Some(1333.0));
        assert_eq!(length("1in"), Some(96.0));
        assert_eq!(length("25.4mm").map(|v| v.round()), Some(96.0));
        assert_eq!(length("50%"), None, "percentages need a viewport");
        assert_eq!(length("1e-5px"), Some(1e-5));
        assert_eq!(length("12em"), Some(192.0));
        assert_eq!(length("garbage"), None);
    }

    #[test]
    fn colours_read_hex_rgb_and_names() {
        assert_eq!(color("#f00"), Some(Rgba::rgb(255, 0, 0)));
        assert_eq!(color("#ff000080"), Some(Rgba::rgba(255, 0, 0, 128)));
        assert_eq!(color("#0000"), Some(Rgba::rgba(0, 0, 0, 0)));
        assert_eq!(color("rgb(0, 128, 255)"), Some(Rgba::rgb(0, 128, 255)));
        assert_eq!(color("rgba(255 255 255 0.5)"), Some(Rgba::rgba(255, 255, 255, 128)));
        assert_eq!(color("rgb(100%, 0%, 0%)"), Some(Rgba::rgb(255, 0, 0)));
        assert_eq!(named("White"), Some(Rgba::WHITE));
        assert_eq!(color("chartreuse"), None, "unknown names are not guessed");
    }

    #[test]
    fn transforms_compose_in_written_order() {
        let m = transform("translate(10 20) scale(2)");
        assert_eq!(m * kurbo::Point::new(0.0, 0.0), kurbo::Point::new(10.0, 20.0));
        assert_eq!(m * kurbo::Point::new(1.0, 1.0), kurbo::Point::new(12.0, 22.0));
        let r = transform("rotate(90 5 5)");
        let p = r * kurbo::Point::new(6.0, 5.0);
        assert!((p.x - 5.0).abs() < 1e-9 && (p.y - 6.0).abs() < 1e-9, "{p:?}");
        assert_eq!(transform("matrix(1 0 0 1 3 4)") * kurbo::Point::new(0.0, 0.0), kurbo::Point::new(3.0, 4.0));
        assert_eq!(transform("nonsense"), Affine::IDENTITY);
    }

    #[test]
    fn a_comma_between_operations_is_a_separator() {
        let a = transform("translate(10 20),scale(2)");
        let b = transform("translate(10 20) , scale(2)");
        assert_eq!(a, transform("translate(10 20) scale(2)"));
        assert_eq!(b, a);
    }

    #[test]
    fn skew_takes_the_tangent_of_its_angle() {
        // skewX(45) maps (x, y) to (x + y, y).
        let m = transform("skewX(45)");
        let p = m * kurbo::Point::new(0.0, 10.0);
        assert!((p.x - 10.0).abs() < 1e-9 && (p.y - 10.0).abs() < 1e-9, "{p:?}");
        let m = transform("skewY(45)");
        let p = m * kurbo::Point::new(10.0, 0.0);
        assert!((p.x - 10.0).abs() < 1e-9 && (p.y - 10.0).abs() < 1e-9, "{p:?}");
    }
}
