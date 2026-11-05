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
        let w = (self.x + self.w).min(r[0] + r[2]) - self.x.max(r[0]);
        let h = (self.y + self.h).min(r[1] + r[3]) - self.y.max(r[1]);
        if w > 0 && h > 0 { i64::from(w) * i64::from(h) } else { 0 }
    }

    /// A window of `size` centred on this display, shrunk to leave a margin around it.
    fn centre(&self, size: [i32; 2]) -> [i32; 4] {
        let w = size[0].clamp(1, (self.w - 2 * MARGIN).max(1));
        let h = size[1].clamp(1, (self.h - 2 * MARGIN).max(1));
        [self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h]
    }
}

/// Room kept around a window placed on a display, in physical pixels (title bar, taskbar).
const MARGIN: i32 = 40;
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
    /// display that is no longer connected falls back to the primary display, and Automatic
    /// (neither saved) uses the editor's. Either way the other, then the first display, when one
    /// is not known.
    pub fn first_choice(&self, index: Option<usize>, id: Option<&Display>) -> usize {
        let valid = |i: &usize| *i < self.list.len();
        let (primary, editor) = (self.primary.filter(valid), self.editor.filter(valid));
        if index.is_none() && id.is_none() {
            return editor.or(primary).unwrap_or(0);
        }
        self.find(index, id).or(primary).or(editor).unwrap_or(0)
    }

    /// Can the window at `r` still be grabbed: is enough of its title bar on one display?
    fn reachable(&self, r: [i32; 4]) -> bool {
        let bar = [r[0], r[1], r[2], TITLE_BAR];
        self.list.iter().any(|d| d.overlap(bar) >= i64::from(GRAB.min(r[2])) * i64::from(TITLE_BAR))
    }

    /// Where Presenter View's window opens: where it was left (`saved`) while its title bar can
    /// still be grabbed, even over the show (it stays on top); else (a display gone, a resolution
    /// changed) centred on a display other than the show's when there is one, with the saved size
    /// or `size`. `None` leaves it to the system.
    pub fn presenter(&self, saved: Option<[i32; 4]>, show: Option<usize>, size: [i32; 2]) -> Option<[i32; 4]> {
        let saved = saved.filter(|r| r[2] > 0 && r[3] > 0);
        if self.list.is_empty() || saved.is_some_and(|r| self.reachable(r)) {
            return saved;
        }
        let d = self.other(show).unwrap_or(0);
        let size = saved.map_or(size, |r| [r[2], r[3]]);
        self.list.get(d).map(|d| d.centre(size))
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
/// or maximize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorRestore {
    pub place: EditorPlace,
    step: u8,
    waited: u8,
}

impl EditorRestore {
    /// Frames to wait for a window to leave full screen before moving it anyway.
    pub const WAIT: u8 = 30;

    pub fn new(place: EditorPlace) -> Self {
        EditorRestore { place, step: 0, waited: 0 }
    }

    /// The next step, given whether the window still reports full screen. `None`: done.
    pub fn next(&mut self, fullscreen: bool) -> Option<RestoreStep> {
        let [x, y, w, h] = self.place.rect;
        let step = match self.step {
            0 => RestoreStep::LeaveFullScreen,
            1 if fullscreen && self.waited < Self::WAIT => {
                self.waited += 1;
                return Some(RestoreStep::Wait);
            }
            1 => RestoreStep::Move([x, y]),
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
        assert_eq!(r.next(true), Some(RestoreStep::LeaveFullScreen));
        // Moved only once the system says the window has left full screen.
        assert_eq!(r.next(true), Some(RestoreStep::Wait));
        assert_eq!(r.next(false), Some(RestoreStep::Move([100, 80])));
        assert_eq!(r.next(false), Some(RestoreStep::Size([1600, 900])));
        assert_eq!(r.next(false), None);
        assert_eq!(r.next(false), None);
        // Maximized: moved to its display, then maximized there.
        let mut r = EditorRestore::new(EditorPlace { maximized: true, ..place });
        assert_eq!(r.next(false), Some(RestoreStep::LeaveFullScreen));
        assert_eq!(r.next(false), Some(RestoreStep::Move([100, 80])));
        assert_eq!(r.next(false), Some(RestoreStep::Maximize));
        assert_eq!(r.next(false), None);
        // A system that never reports leaving full screen does not hold it forever.
        let mut r = EditorRestore::new(place);
        r.next(true);
        let waits = std::iter::from_fn(|| r.next(true)).take_while(|s| *s == RestoreStep::Wait).count();
        assert_eq!(waits, usize::from(EditorRestore::WAIT));
        assert_eq!(r.next(true), Some(RestoreStep::Size([1600, 900])), "moved after the wait");
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
        assert_eq!(d.first_choice(None, None), 0, "the editor's display");
        assert_eq!(d.first_choice(Some(1), None), 1);
        d.editor = Some(1);
        assert_eq!(d.first_choice(None, None), 1);
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
        assert_eq!(d.first_choice(None, None), 0, "Automatic");
    }

    #[test]
    fn presenter_view_comes_back_where_it_was_left() {
        let d = two();
        let left = [100, 120, 900, 600];
        assert_eq!(d.presenter(Some(left), Some(1), [1100, 700]), Some(left));
        // On the show's display too: it stays on top of the show.
        assert_eq!(d.presenter(Some([2000, 50, 900, 600]), Some(1), [1100, 700]), Some([2000, 50, 900, 600]));
        // Nothing saved, or saved on a display that is gone: centred on another display.
        assert_eq!(d.presenter(None, Some(1), [1100, 700]), Some([410, 190, 1100, 700]));
        assert_eq!(d.presenter(Some([-3000, 0, 800, 500]), Some(1), [1100, 700]), Some([560, 290, 800, 500]));
        // Left almost off screen after a resolution change, or with its title bar above the top:
        // it can't be grabbed, so it is centred again.
        assert_eq!(d.presenter(Some([3199, 100, 900, 600]), Some(1), [1100, 700]), Some([510, 240, 900, 600]));
        assert_eq!(d.presenter(Some([100, -200, 900, 600]), Some(1), [1100, 700]), Some([510, 240, 900, 600]));
        // Enough of its title bar shows: it stays where it is.
        assert_eq!(d.presenter(Some([1700, 100, 900, 600]), Some(1), [1100, 700]), Some([1700, 100, 900, 600]));
        // Larger than the display: shrunk to fit.
        assert_eq!(d.presenter(None, Some(0), [4000, 3000]), Some([1960, 40, 1200, 640]));
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
