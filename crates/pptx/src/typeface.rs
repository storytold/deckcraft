//! A typeface name that carries a weight (`Calibri Light`, `Segoe UI Semibold`).
//!
//! PowerPoint stores Regular, Bold and Italic as the family plus `b`/`i`. Any other face is the
//! family and the style in the typeface string.

/// Split `Calibri Light` into `("Calibri", Some("Light"))`. A plain family, a theme reference
/// (`+mn-lt`) and names like `Arial Black` stay whole.
pub fn split_face(typeface: &str) -> (String, Option<String>) {
    let words: Vec<&str> = typeface.split_whitespace().collect();
    if words.len() < 2 || typeface.starts_with('+') {
        return (typeface.to_string(), None);
    }
    let last = words[words.len() - 1];
    let prev = words[words.len() - 2];
    let style_len = if is_slant(last) && (is_weight(prev) || is_weight_pair(words.get(words.len().wrapping_sub(3)).copied().unwrap_or(""), prev)) {
        if words.len() >= 3 && is_weight_pair(words[words.len() - 3], prev) { 3 } else { 2 }
    } else if is_weight(last) {
        1
    } else if is_weight_pair(prev, last) {
        2
    } else {
        return (typeface.to_string(), None);
    };
    if style_len >= words.len() {
        return (typeface.to_string(), None);
    }
    let family = words[..words.len() - style_len].join(" ");
    if family.is_empty() {
        return (typeface.to_string(), None);
    }
    (family, Some(words[words.len() - style_len..].join(" ")))
}

/// A face other than Regular, Bold, Italic or Bold Italic, so the weight lives in the typeface name.
pub fn is_named_style(style: &str) -> bool {
    !style.trim().is_empty() && !is_basic(style)
}

/// The typeface PowerPoint writes for a family and an optional named face.
pub fn join_face(family: &str, style: Option<&str>) -> String {
    match style.filter(|s| is_named_style(s)) {
        Some(style) if !family.is_empty() && !family.starts_with('+') => format!("{family} {style}"),
        _ => family.to_string(),
    }
}

fn is_basic(style: &str) -> bool {
    matches!(norm(style).as_str(), "" | "regular" | "normal" | "roman" | "bold" | "italic" | "oblique" | "bolditalic" | "italicbold" | "boldoblique")
}

fn is_slant(word: &str) -> bool {
    matches!(norm(word).as_str(), "italic" | "oblique")
}

fn is_weight(word: &str) -> bool {
    matches!(
        norm(word).as_str(),
        "thin"
            | "hairline"
            | "extralight"
            | "ultralight"
            | "semilight"
            | "light"
            | "medium"
            | "book"
            | "semibold"
            | "demibold"
            | "extrabold"
            | "ultrabold"
    )
}

fn is_weight_pair(a: &str, b: &str) -> bool {
    let a = norm(a);
    let b = norm(b);
    (b == "bold" && matches!(a.as_str(), "semi" | "demi" | "extra" | "ultra")) || (b == "light" && matches!(a.as_str(), "extra" | "ultra" | "semi"))
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weight_in_the_typeface_splits_off_the_family() {
        assert_eq!(split_face("Calibri Light"), ("Calibri".into(), Some("Light".into())));
        assert_eq!(split_face("Segoe UI Semibold"), ("Segoe UI".into(), Some("Semibold".into())));
        assert_eq!(split_face("Segoe UI Semi Bold"), ("Segoe UI".into(), Some("Semi Bold".into())));
        assert_eq!(split_face("Source Sans 3 Light Italic"), ("Source Sans 3".into(), Some("Light Italic".into())));
        assert_eq!(split_face("Arial"), ("Arial".into(), None));
        assert_eq!(split_face("Arial Black"), ("Arial Black".into(), None));
        assert_eq!(split_face("Arial Bold"), ("Arial Bold".into(), None));
        assert_eq!(split_face("+mn-lt"), ("+mn-lt".into(), None));
        assert_eq!(join_face("Inter", Some("Light")), "Inter Light");
        assert_eq!(join_face("Inter", Some("Bold")), "Inter");
        assert_eq!(join_face("+mn-lt", Some("Light")), "+mn-lt");
    }
}
