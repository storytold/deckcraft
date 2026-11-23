//! Which display the slide show and Presenter View use. Set Up Slide Show chooses the show's display,
//! and Presenter View opens in a window that stays on top of a full-screen show and comes back
//! where it was last left. Swap Displays trades them.
//!
//! The desktop app reports the displays ([`crate::SlideApp::displays`], read through
//! [`crate::Services::displays`] when Set Up Slide Show opens and when a show starts) in physical
//! pixels, in the order `egui::ViewportCommand::SetMonitor` counts them. Window rectangles are
//! `[x, y, w, h]`: the outer position and the inner size, in physical pixels.

use serde::{Deserialize, Serialize};

/// A display: the system's name for it and its area on the desktop, in physical pixels. Saved
/// with the show display (`showDisplayId` in `ui.json`) to find the same screen again after
/// displays are re-plugged or reordered ([`Displays::find`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Display {
    /// `\\.\DISPLAY2` on Windows, the product name on macOS, the output (`HDMI-1`) on Linux;
    /// `None` where the system gives none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Display {
    /// How much of `r` lies on this display, in square pixels.
    fn overlap(&self, r: [i32; 4]) -> i64 {
        let w = (i64::from(self.x) + i64::from(self.w)).min(i64::from(r[0]) + i64::from(r[2])) - i64::from(self.x.max(r[0]));
        let h = (i64::from(self.y) + i64::from(self.h)).min(i64::from(r[1]) + i64::from(r[3])) - i64::from(self.y.max(r[1]));
        if w > 0 && h > 0 { w * h } else { 0 }
    }

    /// The largest window that fits on this display: a margin around it, and at most
    /// [`FIT_PERCENT`] of each side (a small display, such as a 1024 × 768 projector).
    fn room(&self) -> [i32; 2] {
        let side = |n: i32| {
            let share = i32::try_from(i64::from(n) * i64::from(FIT_PERCENT) / 100).unwrap_or(n);
            n.saturating_sub(2 * MARGIN).min(share).max(1)
        };
        [side(self.w), side(self.h)]
    }

    /// A window of `size` centred on this display, shrunk to fit it ([`Self::room`]).
    fn centre(&self, size: [i32; 2]) -> [i32; 4] {
        let [rw, rh] = self.room();
        let (w, h) = (size[0].clamp(1, rw), size[1].clamp(1, rh));
        [self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h]
    }

    /// The window at `r`, shrunk to fit this display ([`Self::room`]) and then moved onto it when it
    /// was too large; a window that fits stays where it is.
    fn fit(&self, r: [i32; 4]) -> [i32; 4] {
        let [rw, rh] = self.room();
        if r[2] <= rw && r[3] <= rh {
            return r;
        }
        let (w, h) = (r[2].clamp(1, rw), r[3].clamp(1, rh));
        // `max` then `min`, not `clamp`: a display narrower than one pixel must not panic.
        let x = r[0].min(self.x.saturating_add(self.w).saturating_sub(w)).max(self.x);
        let y = r[1].min(self.y.saturating_add(self.h).saturating_sub(h)).max(self.y);
        [x, y, w, h]
    }
}

/// Room kept around a window placed on a display, in physical pixels (title bar, taskbar).
const MARGIN: i32 = 40;
/// The most of a display's width or height a placed window takes, in percent.
const FIT_PERCENT: i32 = 90;
/// A window's title bar height, and how much of its width must show to grab it (physical pixels).
const TITLE_BAR: i32 = 32;
const GRAB: i32 = 160;

/// The displays, the one the editor window is on and the system's primary display.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Displays {
    pub list: Vec<Display>,
    pub editor: Option<usize>,
    /// `None` where the system does not say (Wayland).
    pub primary: Option<usize>,
}

impl Displays {
    /// The display most of `r` is on.
    fn under(&self, r: [i32; 4]) -> Option<usize> {
        let (i, area) = self.list.iter().enumerate().map(|(i, d)| (i, d.overlap(r))).max_by_key(|&(i, a)| (a, std::cmp::Reverse(i)))?;
        (area > 0).then_some(i)
    }

    /// The display under the centre of the window at `r`.
    fn under_centre(&self, r: [i32; 4]) -> Option<usize> {
        self.under([r[0].saturating_add(r[2] / 2), r[1].saturating_add(r[3] / 2), 1, 1])
    }

    /// A display other than `not`, the editor's first.
    fn other(&self, not: Option<usize>) -> Option<usize> {
        self.editor.filter(|&e| Some(e) != not && e < self.list.len()).or_else(|| (0..self.list.len()).find(|&i| Some(i) != not))
    }

    /// Where the saved show display is now: `index` into the list when it was chosen, and `id`,
    /// the display it was then. A display is found again by its name, its area telling apart
    /// displays that share a name; one without a name, or saved without a name, by its area. The
    /// index alone is used for a choice saved before `id` was (old `ui.json` files) and for a
    /// display with no name whose area changed. `None`: Automatic, or no longer connected.
    pub fn find(&self, index: Option<usize>, id: Option<&Display>) -> Option<usize> {
        let index = index.filter(|&i| i < self.list.len());
        let Some(id) = id else { return index };
        let area = |d: &Display| (d.x, d.y, d.w, d.h) == (id.x, id.y, id.w, id.h);
        if id.name.is_some() {
            let named: Vec<usize> = (0..self.list.len()).filter(|&i| self.list[i].name == id.name).collect();
            if let [only] = named[..] {
                return Some(only);
            }
            if let Some(&first) = named.first() {
                let same_area = named.iter().copied().find(|&i| area(&self.list[i]));
                return same_area.or(index.filter(|i| named.contains(i))).or(Some(first));
            }
        }
        // A display with another name is another screen, even where the saved one was.
        let by_area = self.list.iter().position(|d| (d.name.is_none() || id.name.is_none()) && area(d));
        by_area.or(index.filter(|_| id.name.is_none()))
    }

    /// The show's display: the saved one ([`Self::find`]) while it is still there; a saved
    /// display that is no longer connected falls back to the primary display, then the editor's.
    /// Automatic (neither saved) uses a display other than the editor's, so the show and
    /// Presenter View (which opens on the editor's) do not share one (#2004): a display that is
    /// not the primary one first, as the editor is usually on the primary display. Where the
    /// editor's display is not known, the first display that is not the primary one.
    pub fn first_choice(&self, index: Option<usize>, id: Option<&Display>) -> usize {
        let valid = |i: &usize| *i < self.list.len();
        let (primary, editor) = (self.primary.filter(valid), self.editor.filter(valid));
        if index.is_none() && id.is_none() {
            let others = || (0..self.list.len()).filter(|&i| Some(i) != editor);
            return others().find(|&i| Some(i) != primary).or_else(|| others().next()).or(editor).unwrap_or(0);
        }
        self.find(index, id).or(primary).or(editor).unwrap_or(0)
    }

    /// Can the window at `r` still be grabbed: is enough of its title bar on one display?
    fn reachable(&self, r: [i32; 4]) -> bool {
        let bar = [r[0], r[1], r[2], TITLE_BAR];
        self.list.iter().any(|d| d.overlap(bar) >= i64::from(GRAB.min(r[2])) * i64::from(TITLE_BAR))
    }

    /// Where Presenter View's window opens: where it was left (`saved`) while its title bar can
    /// still be grabbed and it is not over the show's display (it would cover the show there, #2004;
    /// with one display it stays over the show, on top); else (a display gone, a resolution changed,
    /// left over the show) centred on a display other than the show's when there is one, with the
    /// saved size or `size`. Either way no larger than its display ([`Display::fit`]). `None`
    /// leaves it to the system.
    pub fn presenter(&self, saved: Option<[i32; 4]>, show: Option<usize>, size: [i32; 2]) -> Option<[i32; 4]> {
        let saved = saved.filter(|r| r[2] > 0 && r[3] > 0);
        if self.list.is_empty() {
            return saved;
        }
        let over_show = |r: [i32; 4]| self.list.len() > 1 && show.is_some() && self.under_centre(r) == show;
        if let Some(r) = saved.filter(|&r| self.reachable(r) && !over_show(r)) {
            return Some(self.under_centre(r).or_else(|| self.under(r)).and_then(|d| self.list.get(d)).map_or(r, |d| d.fit(r)));
        }
        let d = self.other(show).unwrap_or(0);
        let size = saved.map_or(size, |r| [r[2], r[3]]);
        self.list.get(d).map(|d| d.centre(size))
    }

    /// Whether most of the window at `r` is on display `d`.
    pub fn on(&self, d: usize, r: [i32; 4]) -> bool {
        self.under_centre(r) == Some(d)
    }

    /// A point just inside display `d`, its top-left corner and a margin, to move a window onto it.
    pub fn inside(&self, d: usize) -> Option<[i32; 2]> {
        self.list.get(d).map(|s| [s.x.saturating_add(MARGIN), s.y.saturating_add(MARGIN)])
    }

    /// Whether a full-screen window with its outer corner at `at` and inner size `size` (physical
    /// pixels) fills display `d`. A display not in the list counts as filled.
    pub fn fills(&self, d: usize, at: [i32; 2], size: [i32; 2]) -> bool {
        let Some(s) = self.list.get(d) else { return true };
        let near = |a: i32, b: i32| a.abs_diff(b) <= 4;
        near(at[0], s.x) && near(at[1], s.y) && near(size[0], s.w) && near(size[1], s.h)
    }

    /// Swap even when the backend omits Presenter View's outer rectangle: use its planned
    /// placement, or the normal display choice and default physical size.
    pub fn swap_or_default(&self, show: usize, presenter: Option<[i32; 4]>, size: [i32; 2]) -> Option<(usize, [i32; 4])> {
        let at = presenter.or_else(|| self.presenter(None, Some(show), size))?;
        self.swap(show, at)
    }

    /// Swap Displays: the show moves to the display Presenter View is on (the next one when they
    /// share one) and Presenter View, keeping its size, to the display the show leaves.
    pub fn swap(&self, show: usize, presenter: [i32; 4]) -> Option<(usize, [i32; 4])> {
        let n = self.list.len();
        if n < 2 || show >= n {
            return None;
        }
        let to = self.under(presenter).filter(|&d| d != show).unwrap_or((show + 1) % n);
        Some((to, self.list[show].centre([presenter[2], presenter[3]])))
    }
}

/// One step of moving a full-screen show to another display ([`ShowMove::next`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveStep {
    /// `ViewportCommand::Fullscreen(false)`.
    LeaveFullScreen,
    /// Nothing this frame.
    Wait,
    /// `ViewportCommand::OuterPosition` onto the display ([`Displays::inside`]).
    Move(usize),
    /// `ViewportCommand::SetMonitor`: full screen on that display.
    Enter(usize),
}

/// Moves a full-screen show to another display (Swap Displays): leave full screen, move the window
/// onto the new display, then go full screen there. On macOS a window already full screen does not
/// move when asked to go full screen on another display (winit only records the new display), and
/// one that has just left full screen goes back to its frame from before, which can still be on
/// the old display: asked to go full screen on the new one from there, it does so on the old one
/// again (#2004). So the window is moved first, and asked again until the system says it is there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShowMove {
    pub to: usize,
    stage: u8,
    since: f64,
    moved: f64,
}

impl ShowMove {
    /// Seconds to let the window leave full screen (the system's animation) before moving it.
    pub const LEAVE_SECS: f64 = 0.5;
    /// Seconds between asking again to move it, and the most it waits at each stage.
    pub const RETRY_SECS: f64 = 0.25;
    pub const GIVE_UP_SECS: f64 = 2.0;

    pub fn new(to: usize) -> Self {
        ShowMove { to, stage: 0, since: 0.0, moved: 0.0 }
    }

    /// The next step at time `now` (seconds), given whether the window still reports full screen
    /// and whether it is on the new display. `None`: done.
    pub fn next(&mut self, now: f64, fullscreen: bool, on_target: bool) -> Option<MoveStep> {
        let waited = now - self.since;
        let step = match self.stage {
            0 => MoveStep::LeaveFullScreen,
            1 if (fullscreen || waited < Self::LEAVE_SECS) && waited < Self::GIVE_UP_SECS => return Some(MoveStep::Wait),
            1 => {
                self.moved = now;
                MoveStep::Move(self.to)
            }
            2 if !on_target && waited < Self::GIVE_UP_SECS => {
                if now - self.moved < Self::RETRY_SECS {
                    return Some(MoveStep::Wait);
                }
                self.moved = now;
                return Some(MoveStep::Move(self.to));
            }
            2 => MoveStep::Enter(self.to),
            _ => return None,
        };
        self.stage += 1;
        self.since = now;
        Some(step)
    }
}

/// The editor window before a show took it full screen on another display: its outer position and
/// inner size in physical pixels, and whether it was maximized. Put back when the show ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorPlace {
    pub rect: [i32; 4],
    pub maximized: bool,
}

impl EditorPlace {
    /// Where the window of `v` is, at `ppp` physical pixels per point. `None` where the system does
    /// not say, or for a window already full screen or minimized (there is nothing to put back).
    pub fn of(v: &egui::ViewportInfo, ppp: f32) -> Option<EditorPlace> {
        if v.fullscreen == Some(true) || v.minimized == Some(true) || !(ppp.is_finite() && ppp > 0.0) {
            return None;
        }
        let (outer, inner) = (v.outer_rect?, v.inner_rect?);
        let px = |v: f32| (v * ppp).round() as i32;
        let rect = [px(outer.min.x), px(outer.min.y), px(inner.width()), px(inner.height())];
        (rect[2] > 0 && rect[3] > 0).then_some(EditorPlace { rect, maximized: v.maximized == Some(true) })
    }
}

/// One step of putting the editor window back ([`EditorRestore::next`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreStep {
    /// `ViewportCommand::Fullscreen(false)`.
    LeaveFullScreen,
    /// Nothing this frame: the window still reports full screen.
    Wait,
    /// `ViewportCommand::OuterPosition`, physical pixels.
    Move([i32; 2]),
    /// `ViewportCommand::InnerSize`, physical pixels, once the window is on its display (and at its
    /// scale).
    Size([i32; 2]),
    /// `ViewportCommand::Maximized(true)`, once the window is on its display.
    Maximize,
}

/// Puts the editor window back where it was before the show, one step per frame: leave full screen,
/// wait for the system to say it has left (at most [`EditorRestore::WAIT`] frames), move, then size
/// or maximize. Moving is retried until its position is observed, also bounded by WAIT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorRestore {
    pub place: EditorPlace,
    step: u8,
    waited: u8,
}

impl EditorRestore {
    /// Maximum frames for each fullscreen/position acknowledgement before proceeding anyway.
    pub const WAIT: u8 = 30;

    pub fn new(place: EditorPlace) -> Self {
        EditorRestore { place, step: 0, waited: 0 }
    }

    /// The next step, given fullscreen and observed outer position in physical pixels. `None`: done.
    /// Without position observations, retry for at most WAIT frames before sizing anyway.
    pub fn next(&mut self, fullscreen: bool, at: Option<[i32; 2]>) -> Option<RestoreStep> {
        let [x, y, w, h] = self.place.rect;
        let step = match self.step {
            0 => RestoreStep::LeaveFullScreen,
            1 if fullscreen && self.waited < Self::WAIT => {
                self.waited += 1;
                return Some(RestoreStep::Wait);
            }
            1 => {
                self.waited = 0;
                RestoreStep::Move([x, y])
            }
            2 if !at.is_some_and(|[ax, ay]| ax.abs_diff(x) <= 4 && ay.abs_diff(y) <= 4) && self.waited < Self::WAIT => {
                self.waited += 1;
                return Some(RestoreStep::Move([x, y]));
            }
            2 if self.place.maximized => RestoreStep::Maximize,
            2 => RestoreStep::Size([w, h]),
            _ => return None,
        };
        self.step += 1;
        Some(step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_window_is_put_back_after_the_show() {
        let place = EditorPlace { rect: [100, 80, 1600, 900], maximized: false };
        let mut r = EditorRestore::new(place);
        assert_eq!(r.next(true, Some([100, 80])), Some(RestoreStep::LeaveFullScreen));
        // Moved only once the system says the window has left full screen.
        assert_eq!(r.next(true, Some([100, 80])), Some(RestoreStep::Wait));
        assert_eq!(r.next(false, Some([100, 80])), Some(RestoreStep::Move([100, 80])));
        assert_eq!(r.next(false, Some([100, 80])), Some(RestoreStep::Size([1600, 900])));
        assert_eq!(r.next(false, Some([100, 80])), None);
        assert_eq!(r.next(false, Some([100, 80])), None);
        // Maximized: moved to its display, then maximized there.
        let mut r = EditorRestore::new(EditorPlace { maximized: true, ..place });
        assert_eq!(r.next(false, Some([100, 80])), Some(RestoreStep::LeaveFullScreen));
        assert_eq!(r.next(false, Some([100, 80])), Some(RestoreStep::Move([100, 80])));
        assert_eq!(r.next(false, Some([100, 80])), Some(RestoreStep::Maximize));
        assert_eq!(r.next(false, Some([100, 80])), None);
        // A system that never reports leaving full screen does not hold it forever.
        let mut r = EditorRestore::new(place);
        r.next(true, Some([100, 80]));
        let waits = std::iter::from_fn(|| r.next(true, Some([100, 80]))).take_while(|s| *s == RestoreStep::Wait).count();
        assert_eq!(waits, usize::from(EditorRestore::WAIT));
        assert_eq!(r.next(true, Some([100, 80])), Some(RestoreStep::Size([1600, 900])), "moved after the wait");
    }

    #[test]
    fn editor_restore_waits_for_position_before_size_or_maximize() {
        for maximized in [false, true] {
            let mut r = EditorRestore::new(EditorPlace { rect: [100, 80, 1600, 900], maximized });
            assert_eq!(r.next(false, Some([2000, 0])), Some(RestoreStep::LeaveFullScreen));
            assert_eq!(r.next(false, Some([2000, 0])), Some(RestoreStep::Move([100, 80])));
            assert_eq!(r.next(false, Some([2000, 0])), Some(RestoreStep::Move([100, 80])));
            assert_eq!(r.next(false, Some([100, 80])), Some(if maximized { RestoreStep::Maximize } else { RestoreStep::Size([1600, 900]) }));
        }
        let mut r = EditorRestore::new(EditorPlace { rect: [100, 80, 1600, 900], maximized: false });
        r.next(false, None);
        r.next(false, None);
        for _ in 0..EditorRestore::WAIT {
            assert_eq!(r.next(false, None), Some(RestoreStep::Move([100, 80])));
        }
        assert_eq!(r.next(false, None), Some(RestoreStep::Size([1600, 900])));
        assert_eq!(r.next(false, None), None);
    }

    #[test]
    fn extreme_saved_positions_do_not_overflow_or_select_a_display() {
        let d = two();
        for x in [i32::MIN, i32::MAX] {
            let rect = [x, x, 1100, 700];
            assert_eq!(d.under(rect), None);
            assert_eq!(d.under_centre(rect), None);
            assert!(d.presenter(Some(rect), Some(1), [1100, 700]).is_some());
            let state = crate::state::UiState { presenter_window: Some(rect), ..Default::default() };
            assert_eq!(state.sanitized().presenter_window, None);
        }
    }

    #[test]
    fn swap_without_a_reported_presenter_position_still_changes_display() {
        let d = two();
        assert_eq!(d.swap_or_default(1, None, [900, 600]), Some((0, [2110, 60, 900, 600])));
        assert_eq!(d.swap_or_default(0, None, [900, 600]).map(|v| v.0), Some(1));
        assert_eq!(Displays::default().swap_or_default(0, None, [900, 600]), None);
    }

    #[test]
    fn the_editor_place_is_read_in_physical_pixels() {
        let v = egui::ViewportInfo {
            outer_rect: Some(egui::Rect::from_min_size(egui::pos2(-960.0, 20.0), egui::vec2(808.0, 630.0))),
            inner_rect: Some(egui::Rect::from_min_size(egui::pos2(-956.0, 50.0), egui::vec2(800.0, 600.0))),
            maximized: Some(true),
            ..Default::default()
        };
        assert_eq!(EditorPlace::of(&v, 1.5), Some(EditorPlace { rect: [-1440, 30, 1200, 900], maximized: true }));
        // Nothing to put back for a window the system does not place, or already full screen.
        assert_eq!(EditorPlace::of(&egui::ViewportInfo::default(), 1.0), None);
        assert_eq!(EditorPlace::of(&egui::ViewportInfo { fullscreen: Some(true), ..v.clone() }, 1.0), None);
        assert_eq!(EditorPlace::of(&egui::ViewportInfo { minimized: Some(true), ..v.clone() }, 1.0), None);
        assert_eq!(EditorPlace::of(&v, f32::NAN), None);
    }

    /// A 1920×1080 laptop at the origin and a 1280×720 projector to its right.
    fn two() -> Displays {
        Displays {
            list: vec![Display { name: None, x: 0, y: 0, w: 1920, h: 1080 }, Display { name: None, x: 1920, y: 0, w: 1280, h: 720 }],
            editor: Some(0),
            primary: Some(0),
        }
    }

    #[test]
    fn the_saved_display_falls_back_when_disconnected() {
        let mut d = two();
        assert_eq!(d.first_choice(None, None), 1, "Automatic: not the editor's display");
        assert_eq!(d.first_choice(Some(1), None), 1);
        d.editor = Some(1);
        assert_eq!(d.first_choice(None, None), 0);
        // A display that has gone: the primary display, even with the editor on another one.
        assert_eq!(d.first_choice(Some(5), None), 0);
        d.primary = Some(1);
        d.editor = Some(0);
        assert_eq!(d.first_choice(Some(5), None), 1);
        // No primary reported (Wayland), or an out-of-date one: the editor's, else the first.
        d.primary = None;
        assert_eq!(d.first_choice(Some(5), None), 0);
        d.editor = Some(1);
        assert_eq!(d.first_choice(Some(5), None), 1);
        d.primary = Some(9);
        d.editor = Some(7);
        assert_eq!(d.first_choice(Some(5), None), 0);
        assert_eq!(Displays::default().first_choice(Some(3), None), 0);
    }

    fn named(name: &str, x: i32, w: i32, h: i32) -> Display {
        Display { name: Some(name.into()), x, y: 0, w, h }
    }

    #[test]
    fn the_saved_display_is_found_again_after_replugging_or_reordering() {
        let laptop = named(r"\\.\DISPLAY1", 0, 1920, 1080);
        let projector = named(r"\\.\DISPLAY2", 1920, 1280, 720);
        let d = Displays { list: vec![laptop.clone(), projector.clone()], editor: Some(0), primary: Some(0) };
        assert_eq!(d.find(Some(1), Some(&projector)), Some(1));
        // Re-plugged: the system lists it first now. The saved index names the laptop; the name
        // still finds the projector.
        let swapped = Displays { list: vec![projector.clone(), laptop.clone()], editor: Some(1), primary: Some(1) };
        assert_eq!(swapped.find(Some(1), Some(&projector)), Some(0));
        assert_eq!(swapped.first_choice(Some(1), Some(&projector)), 0);
        // Moved to the left of the laptop, at another resolution: the same name, the same screen.
        let moved = Displays { list: vec![laptop.clone(), named(r"\\.\DISPLAY2", -1920, 1920, 1080)], editor: Some(0), primary: Some(0) };
        assert_eq!(moved.find(Some(1), Some(&projector)), Some(1));
        // Unplugged: another screen sits at index 1 and where the projector was, but it has another
        // name, so the show falls back to the primary display instead.
        let other = Displays { list: vec![laptop.clone(), named(r"\\.\DISPLAY3", 1920, 1280, 720)], editor: Some(1), primary: Some(0) };
        assert_eq!(other.find(Some(1), Some(&projector)), None);
        assert_eq!(other.first_choice(Some(1), Some(&projector)), 0);
        assert_eq!(Displays { list: vec![laptop.clone()], editor: Some(0), primary: Some(0) }.find(Some(1), Some(&projector)), None);
    }

    #[test]
    fn displays_sharing_a_name_are_told_apart_by_area_then_index() {
        let a = named("DELL U2720Q", 0, 3840, 2160);
        let b = named("DELL U2720Q", 3840, 3840, 2160);
        let d = Displays { list: vec![b.clone(), a.clone()], editor: Some(0), primary: Some(0) };
        assert_eq!(d.find(Some(0), Some(&a)), Some(1), "by area");
        // Neither area matches any more: the saved index among those with the name, else the first.
        let moved = Displays { list: vec![named("DELL U2720Q", 0, 2560, 1440), named("DELL U2720Q", 2560, 2560, 1440)], ..d.clone() };
        assert_eq!(moved.find(Some(1), Some(&a)), Some(1));
        assert_eq!(moved.find(Some(7), Some(&a)), Some(0));
    }

    #[test]
    fn displays_without_names_are_found_by_area_then_index() {
        let d = two();
        let projector = d.list[1].clone();
        let swapped = Displays { list: vec![d.list[1].clone(), d.list[0].clone()], ..d.clone() };
        assert_eq!(swapped.find(Some(1), Some(&projector)), Some(0), "by area");
        // Its area changed too: only the index is left.
        let resized = Displays { list: vec![d.list[0].clone(), Display { w: 1920, h: 1080, ..projector.clone() }], ..d.clone() };
        assert_eq!(resized.find(Some(1), Some(&projector)), Some(1));
        assert_eq!(resized.find(Some(4), Some(&projector)), None);
        // A choice saved before the display was (old ui.json): the index, as before.
        assert_eq!(d.find(Some(1), None), Some(1));
        assert_eq!(d.find(Some(2), None), None);
        assert_eq!(d.find(None, None), None);
        assert_eq!(d.first_choice(None, None), 1, "Automatic");
    }

    #[test]
    fn presenter_view_comes_back_where_it_was_left() {
        let d = two();
        let left = [100, 120, 900, 600];
        assert_eq!(d.presenter(Some(left), Some(1), [1100, 700]), Some(left));
        // Left over the show's display: it would cover the show (#2004), so it is centred on
        // another display, keeping its size.
        assert_eq!(d.presenter(Some([2000, 50, 900, 600]), Some(1), [1100, 700]), Some([510, 240, 900, 600]));
        // Nothing saved, or saved on a display that is gone: centred on another display.
        assert_eq!(d.presenter(None, Some(1), [1100, 700]), Some([410, 190, 1100, 700]));
        assert_eq!(d.presenter(Some([-3000, 0, 800, 500]), Some(1), [1100, 700]), Some([560, 290, 800, 500]));
        // Left almost off screen after a resolution change, or with its title bar above the top:
        // it can't be grabbed, so it is centred again.
        assert_eq!(d.presenter(Some([3199, 100, 900, 600]), Some(1), [1100, 700]), Some([510, 240, 900, 600]));
        assert_eq!(d.presenter(Some([100, -200, 900, 600]), Some(1), [1100, 700]), Some([510, 240, 900, 600]));
        // Enough of its title bar shows, off the show's display: it stays where it is.
        assert_eq!(d.presenter(Some([1700, 100, 900, 600]), Some(0), [1100, 700]), Some([1700, 100, 900, 600]));
        // Larger than the display: shrunk to fit, at most 90 % of each side.
        assert_eq!(d.presenter(None, Some(0), [4000, 3000]), Some([1984, 40, 1152, 640]));
    }

    #[test]
    fn with_one_display_presenter_view_keeps_its_place_over_the_show() {
        let one = Displays { list: vec![Display { name: None, x: 0, y: 0, w: 1920, h: 1080 }], editor: Some(0), primary: Some(0) };
        assert_eq!(one.presenter(Some([100, 100, 800, 500]), None, [1100, 700]), Some([100, 100, 800, 500]));
        assert_eq!(one.presenter(Some([5000, 100, 800, 500]), None, [1100, 700]), Some([560, 290, 800, 500]));
        // No display list (tests, web): whatever was saved, else the system's choice.
        assert_eq!(Displays::default().presenter(Some([1, 2, 3, 4]), None, [1100, 700]), Some([1, 2, 3, 4]));
        assert_eq!(Displays::default().presenter(None, None, [1100, 700]), None);
    }

    /// A MacBook's built-in display (2880 × 1800 pixels, primary) and an iPad used as a display
    /// (2048 × 1536) to its left, as in #2004.
    fn laptop_and_ipad(editor: usize) -> Displays {
        Displays {
            list: vec![
                Display { name: Some("Built-in Retina Display".into()), x: 0, y: 0, w: 2880, h: 1800 },
                Display { name: Some("iPad".into()), x: -2048, y: 264, w: 2048, h: 1536 },
            ],
            editor: Some(editor),
            primary: Some(0),
        }
    }

    #[test]
    fn automatic_uses_a_display_other_than_the_editors() {
        // #2004: the show filled the editor's display and Presenter View opened on it too.
        assert_eq!(laptop_and_ipad(0).first_choice(None, None), 1);
        assert_eq!(laptop_and_ipad(1).first_choice(None, None), 0);
        // Three displays: not the editor's, and not the primary one where there is a choice.
        let three = |editor| Displays {
            list: [0, 1920, 3840].map(|x| Display { name: None, x, y: 0, w: 1920, h: 1080 }).to_vec(),
            editor: Some(editor),
            primary: Some(0),
        };
        assert_eq!(three(0).first_choice(None, None), 1);
        assert_eq!(three(1).first_choice(None, None), 2);
        assert_eq!(three(2).first_choice(None, None), 1);
        // One display: that one.
        let mut one = laptop_and_ipad(0);
        one.list.truncate(1);
        assert_eq!(one.first_choice(None, None), 0);
        assert_eq!(Displays::default().first_choice(None, None), 0);
    }

    #[test]
    fn automatic_without_the_editors_display_uses_one_that_is_not_primary() {
        let mut d = laptop_and_ipad(0);
        d.editor = None;
        assert_eq!(d.first_choice(None, None), 1);
        d.primary = Some(1);
        assert_eq!(d.first_choice(None, None), 0);
        // An out-of-date editor index counts as not known.
        d.editor = Some(9);
        assert_eq!(d.first_choice(None, None), 0);
        // Nothing known: the first display.
        d.primary = None;
        d.editor = None;
        assert_eq!(d.first_choice(None, None), 0);
    }

    #[test]
    fn presenter_view_left_over_the_show_opens_on_another_display() {
        let d = laptop_and_ipad(0);
        let on_ipad = [-2000, 300, 1800, 1300];
        // The show on the iPad: Presenter View, last left on the iPad, comes to the built-in display.
        assert_eq!(d.presenter(Some(on_ipad), Some(1), [2200, 1400]), Some([540, 250, 1800, 1300]));
        // The show on the built-in display: the same place is fine.
        assert_eq!(d.presenter(Some(on_ipad), Some(0), [2200, 1400]), Some(on_ipad));
        // Mostly over the show, but with its centre off it: kept.
        assert_eq!(d.presenter(Some([-300, 300, 1800, 1300]), Some(1), [2200, 1400]), Some([-300, 300, 1800, 1300]));
    }

    #[test]
    fn presenter_view_fits_a_1024_by_768_display() {
        // The iPad is 1024 × 768 points at 2 pixels per point. Presenter View's 1100 × 700 points
        // (2200 × 1400 pixels at that scale) is wider than it: shrunk to 90 % and centred on it.
        let d = laptop_and_ipad(1);
        assert_eq!(d.first_choice(None, None), 0, "the show on the built-in display");
        assert_eq!(d.presenter(None, Some(0), [2200, 1400]), Some([-1946, 341, 1843, 1382]));
        // Left there at a size too large for it (it was saved on a larger display): shrunk and moved
        // onto it.
        assert_eq!(d.presenter(Some([-2048, 264, 2200, 1400]), Some(0), [2200, 1400]), Some([-2048, 264, 1843, 1382]));
        assert_eq!(d.presenter(Some([-1500, 400, 2200, 1400]), Some(0), [2200, 1400]), Some([-1843, 400, 1843, 1382]));
        // On the built-in display it fits as it is.
        assert_eq!(laptop_and_ipad(0).presenter(None, Some(1), [2200, 1400]), Some([340, 200, 2200, 1400]));
        // A display narrower than the margins does not panic.
        let tiny = Displays { list: vec![Display { name: None, x: 0, y: 0, w: 0, h: 0 }, d.list[0].clone()], editor: Some(0), primary: Some(1) };
        assert_eq!(tiny.presenter(None, Some(1), [2200, 1400]), Some([0, 0, 1, 1]));
    }

    #[test]
    fn a_window_fills_a_display_at_its_origin_and_size() {
        let d = laptop_and_ipad(0);
        assert!(d.fills(1, [-2048, 264], [2048, 1536]));
        assert!(d.fills(1, [-2047, 266], [2046, 1536]), "a few pixels of rounding");
        // Still on the built-in display, or not yet its full size: not yet.
        assert!(!d.fills(1, [0, 0], [2880, 1800]));
        assert!(!d.fills(1, [-2048, 264], [1440, 900]));
        assert!(d.fills(0, [0, 0], [2880, 1800]));
        assert!(d.fills(7, [0, 0], [1, 1]), "a display not listed does not hold Presenter View back");
    }

    #[test]
    fn swap_displays_moves_the_window_before_it_goes_full_screen_again() {
        let mut m = ShowMove::new(0);
        assert_eq!(m.next(10.0, true, false), Some(MoveStep::LeaveFullScreen));
        // The system says it has left at once; the animation still runs.
        assert_eq!(m.next(10.1, false, false), Some(MoveStep::Wait));
        assert_eq!(m.next(10.6, false, false), Some(MoveStep::Move(0)));
        // Still on the old display: asked again, a little later.
        assert_eq!(m.next(10.7, false, false), Some(MoveStep::Wait));
        assert_eq!(m.next(10.9, false, false), Some(MoveStep::Move(0)));
        // There: full screen on it.
        assert_eq!(m.next(11.0, false, true), Some(MoveStep::Enter(0)));
        assert_eq!(m.next(11.1, true, true), None);
        // A system that never reports leaving full screen, or the window never arriving, does not
        // hold the show forever.
        let mut m = ShowMove::new(1);
        m.next(0.0, true, false);
        assert_eq!(m.next(1.9, true, false), Some(MoveStep::Wait));
        assert_eq!(m.next(2.0, true, false), Some(MoveStep::Move(1)));
        assert_eq!(m.next(4.0, false, false), Some(MoveStep::Enter(1)));
    }

    #[test]
    fn a_window_is_on_the_display_under_its_centre() {
        let d = laptop_and_ipad(0);
        // The frame macOS puts back after full screen on the iPad, 1440 points wide: still the iPad.
        assert!(d.on(1, [-2880, 62, 2880, 1474]));
        assert!(d.on(0, [80, 80, 2880, 1474]));
        assert_eq!(d.inside(0), Some([40, 40]));
        assert_eq!(d.inside(1), Some([-2008, 304]));
        assert_eq!(d.inside(2), None);
    }

    #[test]
    fn swap_displays_trades_the_show_and_presenter_view() {
        let d = two();
        // Presenter View on the laptop, the show on the projector: they trade.
        assert_eq!(d.swap(1, [100, 100, 900, 600]), Some((0, [2110, 60, 900, 600])));
        // Both on the laptop: the show goes to the next display, Presenter View stays on the laptop.
        assert_eq!(d.swap(0, [100, 100, 900, 600]), Some((1, [510, 240, 900, 600])));
        let three =
            Displays { list: [0, 1920, 3840].map(|x| Display { name: None, x, y: 0, w: 1920, h: 1080 }).to_vec(), editor: Some(0), primary: Some(0) };
        assert_eq!(three.swap(2, [2000, 100, 900, 600]).map(|s| s.0), Some(1));
        assert_eq!(three.swap(2, [4000, 100, 900, 600]).map(|s| s.0), Some(0));
        assert_eq!(Displays::default().swap(0, [0, 0, 10, 10]), None);
    }
}
