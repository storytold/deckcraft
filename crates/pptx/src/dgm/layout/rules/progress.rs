//! Preserve a changed allocation when its measured text budget is unchanged.
use super::{Choice, HashSet, Placed, Range, charge};

fn same_number(a: f64, b: f64) -> bool {
    (a - b).abs() <= 16. * f64::EPSILON * a.abs().max(b.abs()).max(1.)
}

fn same_budget(a: &Placed<'_>, b: &Placed<'_>) -> bool {
    let text = |p: &Placed<'_>| p.text_rect.map(|t| [t.w, t.h, t.rot]);
    a.node_key == b.node_key
        && a.font == b.font
        && a.secondary_font == b.secondary_font
        && a.min_font == b.min_font
        && a.margins == b.margins
        && a.text_rotation == b.text_rotation
        && same_number(a.rect.w, b.rect.w)
        && same_number(a.rect.h, b.rect.h)
        && match (text(a), text(b)) {
            (None, None) => true,
            (Some(a), Some(b)) => a.into_iter().zip(b).all(|(a, b)| same_number(a, b)),
            _ => false,
        }
}

pub(super) fn retain_endpoint(
    range: &Range,
    choice: &Choice<'_>,
    score: f64,
    before: &[Placed<'_>],
    scope: &HashSet<usize>,
    work: &mut usize,
) -> Result<bool, &'static str> {
    if !matches!(range.key.1.as_str(), "w" | "h")
        || range.is_factor
        || range.limit <= range.start
        || choice.t != 1.
        || choice.score > score + 1e-6
        || before.len() != choice.out.len()
    {
        return Ok(false);
    }
    charge(work, before.len())?;
    // A flat score alone is insufficient: coupled width/height searches can have
    // changing budgets with the same overflow. Leave those joint searches intact.
    Ok(before.iter().zip(&choice.out).filter(|(p, _)| scope.contains(&p.node_key)).all(|(a, b)| same_budget(a, b)))
}
