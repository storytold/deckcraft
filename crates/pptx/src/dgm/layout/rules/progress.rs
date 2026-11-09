//! Ordered dimension growth while the local text remains unresolved.
use super::{Choice, Range};

// Prefer an interior value only when it actually fits. A smaller positive
// overflow does not replace an unresolved dimension rule's larger endpoint.
pub(super) fn choose_growth<'a>(mut probe: impl FnMut(f64) -> Result<Option<Choice<'a>>, &'static str>) -> Result<Option<Choice<'a>>, &'static str> {
    let endpoint = probe(1.)?;
    if endpoint.as_ref().is_some_and(|c| c.score <= 1e-6) {
        return Ok(endpoint);
    }
    for step in 1..16 {
        if let Some(mut candidate) = probe(step as f64 / 16.)?
            && candidate.score <= 1e-6
        {
            candidate.low = (step - 1) as f64 / 16.;
            return Ok(Some(candidate));
        }
    }
    Ok(endpoint)
}

pub(super) fn retain_endpoint(range: &Range, choice: &Choice<'_>) -> bool {
    matches!(range.key.1.as_str(), "w" | "h") && !range.is_factor && range.limit > range.start && choice.t == 1.
}
