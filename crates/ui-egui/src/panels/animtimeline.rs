//! Animation Pane advanced timeline: bar geometry and drag-to-retime maths.
//!
//! Pure functions so the layout and the drag results are unit-tested without a frame. Times
//! come from [`deckcraft_anim::Timeline`], the same schedule the slide show plays.

use deckcraft_anim::{EffectTiming, Sequence};

/// Drags snap to this step (milliseconds), like PowerPoint's 0.05 s granularity.
pub const SNAP_MS: u32 = 50;
/// Shortest duration a drag can produce (milliseconds).
pub const MIN_DURATION_MS: u32 = 50;
/// Upper bound for delay and duration, matching `animation.timing`'s clamp.
pub const MAX_MS: u32 = 600_000;
/// Pixels per second are kept in this range whatever the pane width.
const MIN_PPS: f32 = 8.0;
const MAX_PPS: f32 = 120.0;

/// One effect row of the timeline (first paragraph of a by-paragraph build).
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    /// Index into `slide.animations`.
    pub anim: usize,
    /// Click step in the main sequence, or `None` for a triggered sequence.
    pub step: Option<usize>,
    /// Start within the step, delay included (seconds).
    pub start: f64,
    /// Start to end, all repeats included (seconds).
    pub span: f64,
}

/// One bar per animation, in `slide.animations` order; animations without timing get `None`.
pub fn bars(count: usize, effects: &[EffectTiming]) -> Vec<Option<Bar>> {
    let mut out: Vec<Option<Bar>> = vec![None; count];
    for e in effects {
        let Some(slot) = out.get_mut(e.anim) else { continue };
        if slot.is_some() {
            continue;
        }
        let start = if e.start.is_finite() { e.start.max(0.0) } else { 0.0 };
        let span = if e.span.is_finite() { e.span.max(0.0) } else { 0.0 };
        let step = match e.sequence {
            Sequence::Main => Some(e.step),
            Sequence::Trigger(_) => None,
        };
        *slot = Some(Bar { anim: e.anim, step, start, span });
    }
    out
}

/// Seconds the ruler must show: the longest bar end, at least one second, rounded up.
pub fn ruler_seconds(bars: &[Option<Bar>]) -> u32 {
    let end = bars.iter().flatten().map(|b| b.start + b.span).fold(1.0_f64, f64::max);
    let s = end.ceil().min(f64::from(MAX_MS / 1000));
    if s.is_finite() && s >= 1.0 { s as u32 } else { 1 }
}

/// Pixels per second so `seconds` fit in `width`.
pub fn pixels_per_second(width: f32, seconds: u32) -> f32 {
    let secs = seconds.max(1) as f32;
    let pps = if width.is_finite() { width / secs } else { MIN_PPS };
    pps.clamp(MIN_PPS, MAX_PPS)
}

fn snap(ms: f64) -> u32 {
    if !ms.is_finite() || ms <= 0.0 {
        return 0;
    }
    let snapped = (ms / f64::from(SNAP_MS)).round() * f64::from(SNAP_MS);
    if snapped >= f64::from(MAX_MS) { MAX_MS } else { snapped as u32 }
}

fn delta_ms(dx: f32, pps: f32) -> f64 {
    if !dx.is_finite() || !pps.is_finite() || pps <= 0.0 { 0.0 } else { f64::from(dx) / f64::from(pps) * 1000.0 }
}

/// New delay after dragging a bar's body `dx` pixels.
pub fn dragged_delay(delay_ms: u32, dx: f32, pps: f32) -> u32 {
    snap(f64::from(delay_ms) + delta_ms(dx, pps))
}

/// New duration after dragging a bar's right edge `dx` pixels. The bar shows all `repeats`
/// (auto-reverse doubles each), so the edge moves `factor` times as fast as one iteration.
pub fn dragged_duration(duration_ms: u32, dx: f32, pps: f32, factor: f64) -> u32 {
    let factor = if factor.is_finite() && factor >= 1.0 { factor } else { 1.0 };
    snap(f64::from(duration_ms) + delta_ms(dx, pps) / factor).max(MIN_DURATION_MS)
}

/// Tooltip text for a bar: `Start: 0.5s, End: 2.5s`.
pub fn label(bar: &Bar) -> String {
    format!("Start: {:.2}s, End: {:.2}s", bar.start, bar.start + bar.span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::{AnimClass, ShapeId};

    fn eff(anim: usize, step: usize, start: f64, span: f64) -> EffectTiming {
        EffectTiming {
            anim,
            shape: ShapeId(1),
            paragraph: None,
            sequence: Sequence::Main,
            step,
            start,
            duration: span,
            iterations: 1.0,
            until_next_click: false,
            auto_reverse: false,
            rewind: false,
            span,
            class: AnimClass::Entrance,
        }
    }

    #[test]
    fn first_paragraph_wins_and_missing_are_none() {
        let b = bars(3, &[eff(0, 0, 0.0, 0.5), eff(0, 0, 0.5, 0.5), eff(2, 1, f64::NAN, -1.0), eff(9, 0, 0.0, 1.0)]);
        assert_eq!(b[0], Some(Bar { anim: 0, step: Some(0), start: 0.0, span: 0.5 }));
        assert_eq!(b[1], None);
        assert_eq!(b[2], Some(Bar { anim: 2, step: Some(1), start: 0.0, span: 0.0 }));
    }

    #[test]
    fn ruler_and_scale() {
        assert_eq!(ruler_seconds(&[]), 1);
        assert_eq!(ruler_seconds(&bars(1, &[eff(0, 0, 1.2, 2.0)])), 4);
        assert_eq!(pixels_per_second(200.0, 4), 50.0);
        assert_eq!(pixels_per_second(10_000.0, 1), MAX_PPS);
        assert_eq!(pixels_per_second(f32::NAN, 0), MIN_PPS);
    }

    #[test]
    fn drags_snap_and_clamp() {
        assert_eq!(dragged_delay(0, 25.0, 50.0), 500);
        assert_eq!(dragged_delay(500, -100.0, 50.0), 0);
        assert_eq!(dragged_delay(0, 13.0, 50.0), 250);
        assert_eq!(dragged_delay(0, f32::INFINITY, 50.0), 0);
        assert_eq!(dragged_delay(MAX_MS, 1.0e9, 1.0), MAX_MS);
        assert_eq!(dragged_duration(500, 50.0, 50.0, 1.0), 1500);
        assert_eq!(dragged_duration(500, 50.0, 50.0, 2.0), 1000);
        assert_eq!(dragged_duration(500, -1000.0, 50.0, 1.0), MIN_DURATION_MS);
        assert_eq!(dragged_duration(500, 0.0, 0.0, f64::NAN), 500);
    }

    #[test]
    fn label_text() {
        assert_eq!(label(&Bar { anim: 0, step: None, start: 0.5, span: 2.0 }), "Start: 0.50s, End: 2.50s");
    }
}
