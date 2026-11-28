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

/// How long after the mouse button is let go a resize is snapped, in seconds, when the host can
/// tell whether a button is held.
pub(super) const RELEASED_SECS: f64 = 0.15;

/// How often to look again while a mouse button is held during a resize, in seconds.
const HELD_POLL_SECS: f64 = 0.1;

/// Room left around the window on the monitor (menu bar, Dock, task bar), in points.
const SCREEN_MARGIN: Vec2 = Vec2::new(64.0, 96.0);

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
/// kept. With `max` (the largest allowed inner size) the result is scaled down to fit in it. The
/// show's minimum size gives way to `max` when both can't be met, as with a very tall or very
/// wide slide on a small monitor.
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
    let room = max.map(|max| (max - chrome).max(Vec2::splat(1.0)));
    let shrink = |slide: Vec2| match room {
        Some(room) => {
            let scale = (room.x / slide.x).min(room.y / slide.y);
            if scale.is_finite() && scale < 1.0 { slide * scale } else { slide }
        }
        None => slide,
    };
    slide = shrink(slide);
    let min = SHOW_MIN_INNER_SIZE;
    if slide.x < min.x || slide.y < min.y {
        let scale = (min.x / slide.x).max(min.y / slide.y);
        if scale.is_finite() {
            slide *= scale;
        }
    }
    // The minimum may have pushed one side past the monitor: the monitor wins.
    slide = shrink(slide);
    (chrome + slide).round()
}

/// The window minimum a resize to `size` needs: the show's minimum, lowered to `size` where a
/// very tall or very wide slide on a small monitor asks for less (see [`fitted_inner_size`]).
/// Without it the window manager would hold the window at the minimum, bars and all, and every
/// snap would ask again.
pub(super) fn min_inner_size_for(size: Vec2) -> Vec2 {
    SHOW_MIN_INNER_SIZE.min(size.max(Vec2::splat(1.0)))
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
    /// The inner size the last resize settled on or asked for.
    settled: Vec2,
    changed_at: f64,
    pending: bool,
}

impl WindowFit {
    pub(super) fn new(editor_inner: Option<Vec2>, maximized: bool) -> Self {
        WindowFit { editor_inner, maximized, sized: false, seen: Vec2::ZERO, settled: Vec2::ZERO, changed_at: 0.0, pending: false }
    }

    /// One frame of a browsed show: the window's inner size, the part of it the slide area takes,
    /// the slide's shape and the monitor's size. The first frame sizes the window;
    /// after that a resize is snapped, keeping the side the user changed more. `held` says
    /// whether a mouse button is held anywhere, when the host can tell: during a drag of the
    /// window's edge egui sees no pointer events, so this is how a pause in the drag is told
    /// from its end. While it is held nothing is snapped; once it is let go the snap follows
    /// after [`RELEASED_SECS`]. When the host can't tell, the size must stay the same for
    /// [`SETTLE_SECS`]. The window is kept [`SCREEN_MARGIN`] inside the monitor, except that a
    /// window already on screen inside that margin may stay that large. It is never larger than
    /// the monitor.
    pub(super) fn frame(&mut self, inner: Vec2, slide_area: Vec2, aspect: f32, monitor: Option<Vec2>, held: Option<bool>, now: f64) -> Step {
        let max = monitor.map(|m| (m - SCREEN_MARGIN).max(inner).min(m));
        let on_monitor = max.is_none_or(|m| inner.x <= m.x + TOLERANCE && inner.y <= m.y + TOLERANCE);
        if !self.sized {
            self.sized = true;
            self.seen = inner;
            self.settled = inner;
            self.changed_at = now;
            if fits(slide_area, aspect) && on_monitor {
                return Step::Idle;
            }
            let size = fitted_inner_size(inner, slide_area, aspect, Keep::Inside, max);
            self.settled = size;
            return Step::Resize(size);
        }
        if (inner - self.seen).length() > 0.5 {
            self.seen = inner;
            self.changed_at = now;
            self.pending = true;
        }
        // A window moved onto a smaller monitor, without a resize, is brought back onto it.
        if !on_monitor && !self.pending {
            self.pending = true;
            self.changed_at = now;
        }
        if !self.pending {
            return Step::Idle;
        }
        if held == Some(true) {
            self.changed_at = now;
            return Step::Wait(HELD_POLL_SECS);
        }
        let settle = if held.is_some() { RELEASED_SECS } else { SETTLE_SECS };
        let left = self.changed_at + settle - now;
        if left > 0.0 {
            return Step::Wait(left);
        }
        self.pending = false;
        let before = std::mem::replace(&mut self.settled, inner);
        if fits(slide_area, aspect) && on_monitor {
            return Step::Idle;
        }
        let moved = inner - before;
        let keep = if moved.y.abs() > moved.x.abs() { Keep::Height } else { Keep::Width };
        let size = fitted_inner_size(inner, slide_area, aspect, keep, max);
        self.settled = size;
        Step::Resize(size)
    }
}
