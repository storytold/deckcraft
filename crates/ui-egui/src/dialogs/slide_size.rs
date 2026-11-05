use super::{Dialog, exact_of, field_of};

/// A Slide Size field set to `pt`: two decimals of the unit shown, the exact points kept beside.
pub(super) fn put_slide_dim(d: &mut Dialog, key: &str, pt: f64, ppu: f64) {
    d.fields.insert(key.into(), format!("{:.2}", pt / ppu));
    d.fields.insert(format!("{key}Pt"), pt.to_string());
}

/// A Slide Size field in points: the exact value while the text is still its rendering, else
/// the number typed (`default` when it is not one).
pub(super) fn slide_dim_points(d: &Dialog, key: &str, ppu: f64, default: f64) -> f64 {
    let text = field_of(d, key);
    if let Some(pt) = exact_of(d, key)
        && text == format!("{:.2}", pt / ppu)
    {
        return pt;
    }
    text.trim().trim_end_matches(['"', '%', ' ']).parse::<f64>().ok().filter(|v| v.is_finite()).map_or(default, |v| v * ppu)
}

/// The unit's abbreviation in a field label, "Width (cm):".
pub(super) fn unit_abbr(unit: deckcraft_geom::Unit) -> &'static str {
    match unit {
        deckcraft_geom::Unit::Inches => "in",
        deckcraft_geom::Unit::Centimeters => "cm",
        deckcraft_geom::Unit::Points => "pt",
        deckcraft_geom::Unit::Pixels => "px",
    }
}
