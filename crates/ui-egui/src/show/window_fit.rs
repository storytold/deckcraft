//! Browsed by an individual (window): the show window takes the slide's shape, so no black
//! bars frame the slide. The window is sized once when the show starts and snapped back to the
//! slide's shape after the user resizes it. The editor's size comes back when the show ends.

use egui::Vec2;

/// The smallest inner size while a browsed show runs. The editor's minimum is taller than a
/// 16:9 slide at that width, so it would keep the bars.
pub(super) const SHOW_MIN_INNER_SIZE: Vec2 = Vec2::new(320.0, 180.0);

/// How long the window size must stay the same before it is snapped, in seconds. A snap during
/// a drag would fight the user's hand.
pub(super) const SETTLE_SECS: f64 = 0.3;

/// Room left around the window on the monitor (menu bar, Dock, task bar), in points.
pub(super) const SCREEN_MARGIN: Vec2 = Vec2::new(64.0, 96.0);

/// The slide area may be this far off the slide's shape, in points, before the window is snapped.
const TOLERANCE: f32 = 1.0;

/// Which side of the slide area stays when the window takes the slide's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Keep {
    Width,
    Height,
    /// The largest slide-shaped area inside the current one: the window only shrinks.
    Inside,
}

/// The window's inner size that makes its slide area (`slide_area`, inside `inner`) the slide's
/// shape (`aspect` = width / height). Space outside the slide area, such as a title bar, is
/// kept. With `max` (the monitor size less a margin) the result is scaled down to fit in it.
pub(super) fn fitted_inner_size(inner: Vec2, slide_area: Vec2, aspect: f32, keep: Keep, max: Option<Vec2>) -> Vec2 {
    if !aspect.is_finite() || aspect <= 0.0 || !inner.x.is_finite() || !inner.y.is_finite() {
        return inner;
    }
    let chrome = (inner - slide_area).max(Vec2::ZERO);
    let area = slide_area.max(Vec2::splat(1.0));
    let by_width = Vec2::new(area.x, area.x / aspect);
    let by_height = Vec2::new(area.y * aspect, area.y);
    let mut slide = match keep {
        Keep::Width => by_width,
        Keep::Height => by_height,
        Keep::Inside if by_width.y <= area.y => by_width,
        Keep::Inside => by_height,
    };
    if let Some(max) = max {
        let room = (max - chrome).max(Vec2::splat(1.0));
        let scale = (room.x / slide.x).min(room.y / slide.y);
        if scale.is_finite() && scale < 1.0 {
            slide *= scale;
        }
    }
    let min = SHOW_MIN_INNER_SIZE;
    if slide.x < min.x || slide.y < min.y {
        let scale = (min.x / slide.x).max(min.y / slide.y);
        if scale.is_finite() {
            slide *= scale;
        }
    }
    (chrome + slide).round()
}

/// Whether the slide area already has the slide's shape.
pub(super) fn fits(slide_area: Vec2, aspect: f32) -> bool {
    !aspect.is_finite() || aspect <= 0.0 || (slide_area.y - slide_area.x / aspect).abs() <= TOLERANCE
}

/// What the window should do this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Step {
    Idle,
    /// The size changed recently: look again after this many seconds.
    Wait(f64),
    /// Ask for this inner size.
    Resize(Vec2),
}

/// The editor window's state before a browsed show, and the show window's resize tracking.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowFit {
    /// The editor's inner size, put back when the show ends.
    pub(super) editor_inner: Option<Vec2>,
    /// The editor was maximized; it is maximized again when the show ends.
    pub(super) maximized: bool,
    sized: bool,
    /// The inner size seen on the last frame.
    seen: Vec2,
    /// The inner size before the user's last resize.
    settled: Vec2,
    changed_at: f64,
    pending: bool,
}

impl WindowFit {
    pub(super) fn new(editor_inner: Option<Vec2>, maximized: bool) -> Self {
        WindowFit { editor_inner, maximized, sized: false, seen: Vec2::ZERO, settled: Vec2::ZERO, changed_at: 0.0, pending: false }
    }

    /// One frame of a browsed show: the window's inner size, the part of it the slide area takes,
    /// the slide's shape and the largest allowed inner size. The first frame sizes the window;
    /// after that a resize is snapped once the size has stayed the same for [`SETTLE_SECS`],
    /// keeping the side the user changed more. A window already larger than `max` may stay that large.
    pub(super) fn frame(&mut self, inner: Vec2, slide_area: Vec2, aspect: f32, max: Option<Vec2>, now: f64) -> Step {
        let max = max.map(|m| m.max(inner));
        if !self.sized {
            self.sized = true;
            self.seen = inner;
            self.settled = inner;
            self.changed_at = now;
            if fits(slide_area, aspect) {
                return Step::Idle;
            }
            return Step::Resize(fitted_inner_size(inner, slide_area, aspect, Keep::Inside, max));
        }
        if (inner - self.seen).length() > 0.5 {
            self.seen = inner;
            self.changed_at = now;
            self.pending = true;
        }
        if !self.pending {
            return Step::Idle;
        }
        let left = self.changed_at + SETTLE_SECS - now;
        if left > 0.0 {
            return Step::Wait(left);
        }
        self.pending = false;
        let before = std::mem::replace(&mut self.settled, inner);
        if fits(slide_area, aspect) {
            return Step::Idle;
        }
        let moved = inner - before;
        let keep = if moved.y.abs() > moved.x.abs() { Keep::Height } else { Keep::Width };
        Step::Resize(fitted_inner_size(inner, slide_area, aspect, keep, max))
    }
}
